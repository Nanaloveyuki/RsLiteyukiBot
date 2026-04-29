use super::*;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};

fn temp_password_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("liteyuki-webui-auth-{name}-{}.json", now_ms()));
    path
}

#[test]
// 必要测试
fn load_or_init_creates_json_password_store() {
    let path = temp_password_path("init");
    ensure_webui_password_file(path.as_path()).expect("password file should initialize");

    let content = fs::read_to_string(&path).expect("initialized webui password file should exist");
    let doc: WebUiPasswordDoc =
        serde_json::from_str(content.as_str()).expect("password file should be valid json");

    assert_eq!(doc.version, WEBUI_PASSWORD_VERSION);
    assert!(doc.password_hash.is_none());

    let _ = fs::remove_file(path);
}

#[test]
// 必要测试
fn default_webui_password_store_uses_user_configs_dir() {
    let _lock = process_state_lock();
    let base = temp_password_path("user-config-root");
    let _ = fs::remove_file(&base);
    fs::create_dir_all(&base).expect("test home should be created");

    let _userprofile_guard = EnvVarGuard::set("USERPROFILE", &base);
    let _home_guard = EnvVarGuard::remove("HOME");
    let _password_guard = EnvVarGuard::remove("LY_WEBUI_PASSWORD_PATH");

    let path = resolve_webui_password_store_path();
    assert_eq!(
        path,
        base.join(".liteyuki").join("configs").join("password.json")
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
// 必要测试
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
// 必要测试
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
