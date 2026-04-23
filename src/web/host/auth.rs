use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    HEALTH_ROUTE, extract_header, napcat_err, napcat_ok, napcat_response, parse_json_body,
};

const WEBUI_PASSWORD_FILENAME: &str = "password.json";
const WEBUI_PASSWORD_VERSION: u8 = 1;
const BOOTSTRAP_TOKEN_BYTES: usize = 24;
const SESSION_TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WebUiPasswordDoc {
    #[serde(default = "default_password_doc_version")]
    version: u8,
    #[serde(default)]
    password_hash: Option<String>,
    #[serde(default)]
    password_updated_at_ms: Option<u128>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionOrigin {
    BootstrapToken,
    Password,
}

#[derive(Debug)]
struct WebUiAuthState {
    path: Option<PathBuf>,
    password_hash: Option<String>,
    password_updated_at_ms: Option<u128>,
    bootstrap_login_token: String,
    local_session_token: String,
    issued_sessions: HashMap<String, SessionOrigin>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WebUiAuthStatus {
    #[serde(rename = "passwordConfigured")]
    pub password_configured: bool,
    #[serde(rename = "tokenLoginEnabled")]
    pub token_login_enabled: bool,
}

#[derive(Clone)]
pub(crate) struct WebUiAuthManager {
    inner: Arc<Mutex<WebUiAuthState>>,
}

impl WebUiAuthManager {
    pub(crate) fn load_or_init_default() -> Result<Self, String> {
        let path = resolve_webui_password_store_path();
        ensure_webui_password_file(path.as_path())?;
        let content = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let document: WebUiPasswordDoc = serde_json::from_str(content.as_str())
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
        Ok(Self::from_document(Some(path), document))
    }

    #[cfg(test)]
    pub(crate) fn in_memory_for_tests() -> Self {
        Self::from_document(None, WebUiPasswordDoc::default())
    }

    pub(crate) fn storage_path(&self) -> Option<PathBuf> {
        self.inner
            .lock()
            .expect("webui auth state lock should not be poisoned")
            .path
            .clone()
    }

    pub(crate) fn bootstrap_login_token(&self) -> String {
        self.inner
            .lock()
            .expect("webui auth state lock should not be poisoned")
            .bootstrap_login_token
            .clone()
    }

    pub(crate) fn local_session_token(&self) -> String {
        self.inner
            .lock()
            .expect("webui auth state lock should not be poisoned")
            .local_session_token
            .clone()
    }

    pub(crate) fn status(&self) -> WebUiAuthStatus {
        let state = self
            .inner
            .lock()
            .expect("webui auth state lock should not be poisoned");
        WebUiAuthStatus {
            password_configured: state.password_hash.is_some(),
            token_login_enabled: state.password_hash.is_none(),
        }
    }

    pub(crate) fn is_session_token_valid(&self, token: &str) -> bool {
        let candidate = token.trim();
        if candidate.is_empty() {
            return false;
        }
        let state = self
            .inner
            .lock()
            .expect("webui auth state lock should not be poisoned");
        candidate == state.local_session_token || state.issued_sessions.contains_key(candidate)
    }

    pub(crate) fn login_with_bootstrap_hash(&self, hash: &str) -> Result<String, String> {
        let candidate = hash.trim().to_ascii_lowercase();
        if candidate.is_empty() {
            return Err("token hash is required".to_string());
        }

        let mut state = self
            .inner
            .lock()
            .map_err(|_| "webui auth state lock poisoned".to_string())?;
        if state.password_hash.is_some() {
            return Err("token login is disabled after a password has been configured".to_string());
        }

        let expected = sha256_hex(format!("{}.napcat", state.bootstrap_login_token).as_bytes());
        if candidate != expected {
            return Err("invalid login token".to_string());
        }

        let session = generate_token(SESSION_TOKEN_BYTES);
        state
            .issued_sessions
            .insert(session.clone(), SessionOrigin::BootstrapToken);
        Ok(session)
    }

    pub(crate) fn login_with_password(&self, password: &str) -> Result<String, String> {
        let candidate = normalize_password_candidate(password)?;
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "webui auth state lock poisoned".to_string())?;
        let stored_hash = state
            .password_hash
            .clone()
            .ok_or_else(|| "password login is not configured yet".to_string())?;
        verify_password_hash(candidate.as_str(), stored_hash.as_str())
            .map_err(|_| "password is incorrect".to_string())?;

        let session = generate_token(SESSION_TOKEN_BYTES);
        state
            .issued_sessions
            .insert(session.clone(), SessionOrigin::Password);
        Ok(session)
    }

    pub(crate) fn update_password(
        &self,
        current_session_token: Option<&str>,
        old_password: Option<&str>,
        new_password: &str,
    ) -> Result<(), String> {
        let new_password = validate_new_password(new_password)?;
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "webui auth state lock poisoned".to_string())?;

        match state.password_hash.as_deref() {
            Some(stored_hash) => {
                let old_password = old_password
                    .ok_or_else(|| "current password is required".to_string())
                    .and_then(normalize_password_candidate)?;
                verify_password_hash(old_password.as_str(), stored_hash)
                    .map_err(|_| "current password is incorrect".to_string())?;
            }
            None => {
                let session = current_session_token
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "authentication required".to_string())?;
                if session != state.local_session_token
                    && !state.issued_sessions.contains_key(session)
                {
                    return Err("authentication required".to_string());
                }
            }
        }

        let password_hash = hash_password(new_password.as_str())?;
        state.password_hash = Some(password_hash);
        state.password_updated_at_ms = Some(now_ms());
        state.issued_sessions.clear();
        persist_webui_password_state(&state)
    }

    fn from_document(path: Option<PathBuf>, document: WebUiPasswordDoc) -> Self {
        Self {
            inner: Arc::new(Mutex::new(WebUiAuthState {
                path,
                password_hash: document
                    .password_hash
                    .filter(|value| !value.trim().is_empty()),
                password_updated_at_ms: document.password_updated_at_ms,
                bootstrap_login_token: generate_token(BOOTSTRAP_TOKEN_BYTES),
                local_session_token: generate_token(SESSION_TOKEN_BYTES),
                issued_sessions: HashMap::new(),
            })),
        }
    }
}

fn default_password_doc_version() -> u8 {
    WEBUI_PASSWORD_VERSION
}

fn resolve_webui_password_store_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_WEBUI_PASSWORD_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }

    let user_home = std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("HOME").filter(|value| !value.is_empty()));

    match user_home {
        Some(home) => PathBuf::from(home)
            .join(".liteyuki")
            .join(WEBUI_PASSWORD_FILENAME),
        None => PathBuf::from(".liteyuki").join(WEBUI_PASSWORD_FILENAME),
    }
}

fn ensure_webui_password_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create webui password directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let body = serde_json::to_string_pretty(&WebUiPasswordDoc {
        version: WEBUI_PASSWORD_VERSION,
        password_hash: None,
        password_updated_at_ms: None,
    })
    .map_err(|err| format!("failed to serialize {}: {err}", path.display()))?;
    fs::write(path, body).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn persist_webui_password_state(state: &WebUiAuthState) -> Result<(), String> {
    let Some(path) = state.path.as_ref() else {
        return Ok(());
    };
    let body = serde_json::to_string_pretty(&WebUiPasswordDoc {
        version: WEBUI_PASSWORD_VERSION,
        password_hash: state.password_hash.clone(),
        password_updated_at_ms: state.password_updated_at_ms,
    })
    .map_err(|err| format!("failed to serialize {}: {err}", path.display()))?;
    fs::write(path, body).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn normalize_password_candidate(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("password cannot be empty".to_string());
    }
    if trimmed.len() != raw.len() {
        return Err("password cannot contain leading or trailing spaces".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_new_password(raw: &str) -> Result<String, String> {
    let password = normalize_password_candidate(raw)?;
    if password.len() < 6 {
        return Err("password must be at least 6 characters long".to_string());
    }
    if !password.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return Err("password must contain at least one letter".to_string());
    }
    if !password.chars().any(|ch| ch.is_ascii_digit()) {
        return Err("password must contain at least one number".to_string());
    }
    Ok(password)
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| format!("failed to hash password: {err}"))
}

fn verify_password_hash(password: &str, stored_hash: &str) -> Result<(), String> {
    let parsed_hash = PasswordHash::new(stored_hash)
        .map_err(|err| format!("failed to parse stored password hash: {err}"))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|err| format!("password verification failed: {err}"))
}

fn generate_token(byte_len: usize) -> String {
    let mut bytes = vec![0_u8; byte_len];
    OsRng.fill_bytes(bytes.as_mut_slice());
    URL_SAFE_NO_PAD.encode(bytes)
}

fn sha256_hex(raw: &[u8]) -> String {
    let digest = Sha256::digest(raw);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(super) fn route_auth_api(
    auth: &WebUiAuthManager,
    api_path: &str,
    request: &[u8],
    peer_ip: IpAddr,
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/auth/state" {
        let body = napcat_ok(&auth.status());
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/auth/local-token" {
        if peer_ip.is_loopback() {
            #[derive(Serialize)]
            struct LocalTokenResponse<'a> {
                token: &'a str,
            }
            let local_token = auth.local_session_token();
            let body = napcat_ok(&LocalTokenResponse {
                token: local_token.as_str(),
            });
            return Some(napcat_response(body, is_head));
        }

        let body = napcat_err(403, "Forbidden");
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/auth/login" {
        let body = parse_json_body(request);
        let result = body
            .get("hash")
            .and_then(Value::as_str)
            .ok_or_else(|| "token hash is required".to_string())
            .and_then(|hash| auth.login_with_bootstrap_hash(hash));
        return Some(napcat_response(login_response_payload(result), is_head));
    }

    if api_path == "/auth/login/password" {
        let body = parse_json_body(request);
        let result = body
            .get("password")
            .and_then(Value::as_str)
            .ok_or_else(|| "password is required".to_string())
            .and_then(|password| auth.login_with_password(password));
        return Some(napcat_response(login_response_payload(result), is_head));
    }

    if api_path == "/auth/update_token" || api_path == "/auth/update_password" {
        let body = parse_json_body(request);
        let current_session = bearer_token_from_request(request);
        let old_password = body
            .get("oldPassword")
            .and_then(Value::as_str)
            .or_else(|| body.get("oldToken").and_then(Value::as_str));
        let new_password = body
            .get("newPassword")
            .and_then(Value::as_str)
            .or_else(|| body.get("newToken").and_then(Value::as_str));
        let body = match new_password {
            Some(new_password) => {
                match auth.update_password(current_session.as_deref(), old_password, new_password) {
                    Ok(()) => napcat_ok(&true),
                    Err(err) => napcat_err(-1, err.as_str()),
                }
            }
            None => napcat_err(-1, "new password is required"),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/auth/passkey/generate-registration-options"
        || api_path == "/auth/passkey/verify-registration"
        || api_path == "/auth/passkey/generate-authentication-options"
        || api_path == "/auth/passkey/verify-authentication"
    {
        let body = napcat_err(-1, "Passkey auth is not supported by the current runtime");
        return Some(napcat_response(body, is_head));
    }

    None
}

pub(super) fn public_api_path(path: &str) -> bool {
    matches!(
        path,
        HEALTH_ROUTE
            | "/api/auth/check"
            | "/api/auth/state"
            | "/api/auth/local-token"
            | "/api/auth/login"
            | "/api/auth/login/password"
    )
}

pub(super) fn bearer_token_from_request(request: &[u8]) -> Option<String> {
    let request = String::from_utf8_lossy(request);
    let header = extract_header(&request, "Authorization")?;
    let (scheme, token) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

pub(super) fn unauthorized_response(head_only: bool) -> Vec<u8> {
    napcat_response(napcat_err(401, "Unauthorized"), head_only)
}

pub(super) fn request_is_authorized(auth: &WebUiAuthManager, request: &[u8]) -> bool {
    bearer_token_from_request(request)
        .map(|token| auth.is_session_token_valid(token.as_str()))
        .unwrap_or(false)
}

fn login_response_payload(result: Result<String, String>) -> Vec<u8> {
    #[derive(Serialize)]
    struct AuthResponse {
        #[serde(rename = "Credential")]
        credential: String,
    }

    match result {
        Ok(credential) => napcat_ok(&AuthResponse { credential }),
        Err(err) => napcat_err(-1, err.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_password_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("liteyuki-webui-auth-{name}-{}.json", now_ms()));
        path
    }

    #[test]
    fn load_or_init_creates_json_password_store() {
        let path = temp_password_path("init");
        ensure_webui_password_file(path.as_path()).expect("password file should initialize");

        let content =
            fs::read_to_string(&path).expect("initialized webui password file should exist");
        let doc: WebUiPasswordDoc =
            serde_json::from_str(content.as_str()).expect("password file should be valid json");

        assert_eq!(doc.version, WEBUI_PASSWORD_VERSION);
        assert!(doc.password_hash.is_none());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn bootstrap_token_login_is_disabled_after_password_setup() {
        let manager = WebUiAuthManager::in_memory_for_tests();
        let bootstrap_hash =
            sha256_hex(format!("{}.napcat", manager.bootstrap_login_token()).as_bytes());
        let session = manager
            .login_with_bootstrap_hash(bootstrap_hash.as_str())
            .expect("bootstrap token should authenticate");

        manager
            .update_password(Some(session.as_str()), None, "Pass1234")
            .expect("first password setup should succeed");

        let err = manager
            .login_with_bootstrap_hash(bootstrap_hash.as_str())
            .expect_err("bootstrap token login should be disabled");
        assert!(err.contains("disabled"));
    }

    #[test]
    fn password_login_requires_correct_password() {
        let manager = WebUiAuthManager::in_memory_for_tests();
        let bootstrap_hash =
            sha256_hex(format!("{}.napcat", manager.bootstrap_login_token()).as_bytes());
        let session = manager
            .login_with_bootstrap_hash(bootstrap_hash.as_str())
            .expect("bootstrap token should authenticate");
        manager
            .update_password(Some(session.as_str()), None, "Pass1234")
            .expect("first password setup should succeed");

        let password_session = manager
            .login_with_password("Pass1234")
            .expect("password login should succeed");
        assert!(manager.is_session_token_valid(password_session.as_str()));
        assert!(manager.login_with_password("wrong").is_err());
    }
}
