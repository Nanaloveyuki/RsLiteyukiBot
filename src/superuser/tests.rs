use std::sync::Arc;

use super::*;

fn temp_password_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("rsliteyukibot-{name}-{unique}.yaml"))
}

fn mock_event(user_id: &str) -> SessionEvent {
    SessionEvent {
        event_id: 1,
        topic: Arc::from("adapter.inbound"),
        message: Arc::from("/su"),
        payload: serde_json::json!({
            "_adapter_protocol": "onebot.v11",
            "_adapter_id": "ws-main",
            "message_type": "private",
            "user_id": user_id,
            "sender": {
                "nickname": "tester"
            }
        }),
        timestamp_ms: 0,
        bot_id: Arc::from("bot"),
        session_id: Arc::from(user_id),
        user_id: Arc::from(user_id),
        scope: SessionScope::Private,
    }
}

#[test]
// 必要测试
fn load_or_init_creates_password_file_with_empty_password() {
    let path = temp_password_path("password-init");
    let manager =
        SuperuserManager::load_or_init(path.as_path()).expect("password manager should initialize");

    assert!(path.exists());
    assert!(manager.using_dynamic_password());
    let content = std::fs::read_to_string(&path).expect("password file should be readable");
    assert!(content.contains("password: ''"));

    let _ = std::fs::remove_file(path);
}

#[test]
// 必要测试
fn fixed_password_in_file_overrides_dynamic_password() {
    let path = temp_password_path("password-fixed");
    std::fs::write(&path, "password: fixed-pass\nsuperusers: []\n")
        .expect("password file should be written");

    let manager =
        SuperuserManager::load_or_init(path.as_path()).expect("password manager should initialize");
    assert!(!manager.using_dynamic_password());
    assert!(manager.verify_password("fixed-pass"));
    assert!(!manager.verify_password("wrong"));

    let _ = std::fs::remove_file(path);
}

#[test]
// 必要测试
fn promote_user_persists_superuser_to_file() {
    let path = temp_password_path("password-promote");
    let manager =
        SuperuserManager::load_or_init(path.as_path()).expect("password manager should initialize");
    let event = mock_event("10086");

    let result = manager
        .promote_user(&event)
        .expect("promote should persist superuser");
    assert!(result.added);
    assert!(manager.is_superuser(&event));

    let content = std::fs::read_to_string(&path).expect("password file should be readable");
    assert!(content.contains("user_id: '10086'") || content.contains("user_id: \"10086\""));

    let _ = std::fs::remove_file(path);
}
