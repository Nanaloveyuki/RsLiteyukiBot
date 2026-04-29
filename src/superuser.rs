use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use liteyukibot_core::{SessionEvent, SessionScope};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_PASSWORD_CONFIG_TEMPLATE: &str = "password: ''\nsuperusers: []\n";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct SuperuserRecord {
    pub user_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub adapter_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub adapter_protocol: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nickname: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub card: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default)]
    pub authorized_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PasswordConfigDoc {
    #[serde(default)]
    password: String,
    #[serde(default)]
    superusers: Vec<SuperuserRecord>,
}

#[derive(Debug)]
struct SuperuserState {
    path: Option<PathBuf>,
    configured_password: String,
    runtime_password: String,
    superusers: HashMap<String, SuperuserRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PromoteResult {
    pub added: bool,
}

#[derive(Clone)]
pub(crate) struct SuperuserManager {
    inner: Arc<Mutex<SuperuserState>>,
}

impl SuperuserManager {
    pub(crate) fn load_or_init(path: &Path) -> Result<Self, String> {
        ensure_password_file(path)?;
        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let document: PasswordConfigDoc = serde_yaml::from_str(content.as_str())
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
        Ok(Self::from_document(Some(path.to_path_buf()), document))
    }

    pub(crate) fn in_memory() -> Self {
        Self::from_document(None, PasswordConfigDoc::default())
    }

    pub(crate) fn using_dynamic_password(&self) -> bool {
        let state = self
            .inner
            .lock()
            .expect("superuser state lock should not be poisoned");
        state.configured_password.is_empty()
    }

    pub(crate) fn active_password(&self) -> String {
        self.inner
            .lock()
            .expect("superuser state lock should not be poisoned")
            .runtime_password
            .clone()
    }

    pub(crate) fn verify_password(&self, raw: &str) -> bool {
        let password = raw.trim();
        if password.is_empty() {
            return false;
        }
        let state = self
            .inner
            .lock()
            .expect("superuser state lock should not be poisoned");
        password == state.runtime_password
    }

    pub(crate) fn is_superuser(&self, event: &SessionEvent) -> bool {
        let user_id = event.user_id.trim();
        if !is_user_id_usable(user_id) {
            return false;
        }
        self.inner
            .lock()
            .expect("superuser state lock should not be poisoned")
            .superusers
            .contains_key(user_id)
    }

    pub(crate) fn promote_user(&self, event: &SessionEvent) -> Result<PromoteResult, String> {
        let user_id = event.user_id.trim().to_string();
        if !is_user_id_usable(user_id.as_str()) {
            return Err("event user_id is empty or anonymous; cannot grant superuser".to_string());
        }

        let mut state = self
            .inner
            .lock()
            .map_err(|_| "superuser state lock poisoned".to_string())?;
        let record = build_superuser_record(event);
        let added = state.superusers.insert(user_id, record).is_none();
        persist_superuser_state(&state)?;
        Ok(PromoteResult { added })
    }

    fn from_document(path: Option<PathBuf>, document: PasswordConfigDoc) -> Self {
        let configured_password = document.password.trim().to_string();
        let runtime_password = if configured_password.is_empty() {
            generate_dynamic_password()
        } else {
            configured_password.clone()
        };
        let superusers = document
            .superusers
            .into_iter()
            .filter_map(|entry| {
                let key = entry.user_id.trim().to_string();
                if key.is_empty() {
                    None
                } else {
                    Some((key, entry))
                }
            })
            .collect();
        Self {
            inner: Arc::new(Mutex::new(SuperuserState {
                path,
                configured_password,
                runtime_password,
                superusers,
            })),
        }
    }
}

fn ensure_password_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create password config parent directory {}: {err}",
                parent.display()
            )
        })?;
    }
    std::fs::write(path, DEFAULT_PASSWORD_CONFIG_TEMPLATE)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(())
}

fn persist_superuser_state(state: &SuperuserState) -> Result<(), String> {
    let Some(path) = state.path.as_ref() else {
        return Ok(());
    };

    let mut superusers: Vec<SuperuserRecord> = state.superusers.values().cloned().collect();
    superusers.sort_by(|lhs, rhs| lhs.user_id.cmp(&rhs.user_id));

    let document = PasswordConfigDoc {
        password: state.configured_password.clone(),
        superusers,
    };
    let yaml = serde_yaml::to_string(&document)
        .map_err(|err| format!("failed to serialize {}: {err}", path.display()))?;
    std::fs::write(path, yaml).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn build_superuser_record(event: &SessionEvent) -> SuperuserRecord {
    let sender = event.payload.get("sender").and_then(Value::as_object);
    let scope = event
        .payload
        .get("message_type")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| session_scope_name(&event.scope).to_string());

    SuperuserRecord {
        user_id: event.user_id.to_string(),
        session_id: event.session_id.to_string(),
        scope,
        adapter_id: event
            .payload
            .get("_adapter_id")
            .and_then(value_to_string)
            .unwrap_or_default(),
        adapter_protocol: event
            .payload
            .get("_adapter_protocol")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_default(),
        nickname: sender
            .and_then(|sender| sender.get("nickname"))
            .and_then(value_to_string)
            .unwrap_or_default(),
        card: sender
            .and_then(|sender| sender.get("card"))
            .and_then(value_to_string)
            .unwrap_or_default(),
        role: sender
            .and_then(|sender| sender.get("role"))
            .and_then(value_to_string)
            .unwrap_or_default(),
        authorized_at_ms: now_millis(),
    }
}

fn session_scope_name(scope: &SessionScope) -> &'static str {
    match scope {
        SessionScope::Private => "private",
        SessionScope::Group => "group",
        SessionScope::Guild => "guild",
        SessionScope::ChannelText => "channel_text",
        SessionScope::ChannelCategory => "channel_category",
        SessionScope::ChannelVoice => "channel_voice",
        SessionScope::Other(_) => "other",
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    if let Some(raw) = value.as_str() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_u64() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_i64() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_bool() {
        return Some(raw.to_string());
    }
    None
}

fn is_user_id_usable(user_id: &str) -> bool {
    !user_id.trim().is_empty() && !user_id.trim().eq_ignore_ascii_case("anonymous")
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn generate_dynamic_password() -> String {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        ^ ((std::process::id() as u128) << 32);
    let mut state = seed | 1;
    let mut output = String::new();
    while output.len() < 32 {
        state ^= state << 7;
        state ^= state >> 9;
        state ^= state << 8;
        output.push_str(format!("{:016x}", (state as u64)).as_str());
    }
    output.truncate(32);
    output
}

#[cfg(test)]
#[path = "superuser/tests.rs"]
mod tests;
