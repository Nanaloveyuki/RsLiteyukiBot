use super::*;
use crate::{SessionEvent, SessionScope};
use serde_json::Value;
use std::sync::Arc;

fn mock_onebot_event(message_type: &str, raw_message: &str) -> SessionEvent {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "_adapter_protocol".to_string(),
        Value::String("onebot.v11".to_string()),
    );
    payload.insert(
        "post_type".to_string(),
        Value::String("message".to_string()),
    );
    payload.insert(
        "message_type".to_string(),
        Value::String(message_type.to_string()),
    );
    payload.insert(
        "raw_message".to_string(),
        Value::String(raw_message.to_string()),
    );
    payload.insert("user_id".to_string(), Value::String("10001".to_string()));
    if message_type == "group" {
        payload.insert("group_id".to_string(), Value::String("2333".to_string()));
    }
    SessionEvent {
        event_id: 1,
        topic: Arc::from("adapter.inbound"),
        message: Arc::from(raw_message),
        payload: Value::Object(payload),
        timestamp_ms: 0,
        bot_id: Arc::from("bot"),
        session_id: Arc::from(if message_type == "group" {
            "2333"
        } else {
            "10001"
        }),
        user_id: Arc::from("10001"),
        scope: if message_type == "group" {
            SessionScope::Group
        } else {
            SessionScope::Private
        },
    }
}

#[test]
// 必要测试
fn parse_su_password_argument_supports_slash_and_plain_command() {
    assert_eq!(
        parse_su_password_argument("/su abc123"),
        Some("abc123".to_string())
    );
    assert_eq!(
        parse_su_password_argument("su abc123"),
        Some("abc123".to_string())
    );
    assert_eq!(parse_su_password_argument("/su"), Some(String::new()));
}

#[test]
// 必要测试
fn onebot_private_message_detection_respects_message_type() {
    let private_event = mock_onebot_event("private", "/su secret");
    assert!(is_onebot_private_message(&private_event));

    let group_event = mock_onebot_event("group", "/su secret");
    assert!(!is_onebot_private_message(&group_event));
}
