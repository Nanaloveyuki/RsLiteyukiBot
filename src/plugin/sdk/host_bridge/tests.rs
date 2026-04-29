use super::*;
use serde_json::json;

#[test]
// 必要测试
fn onebot_reply_payload_uses_action_envelope_for_group() {
    let payload = json!({
        "message_type": "group",
        "group_id": 112233
    });
    let built = build_onebot_v11_text_reply_payload(
        payload.as_object().expect("test payload should be object"),
        "hello",
    )
    .expect("group payload should build");

    assert_eq!(
        built.get("action").and_then(Value::as_str),
        Some("send_msg")
    );
    assert_eq!(
        built.get("params").and_then(|v| v.get("group_id")),
        Some(&json!(112233))
    );
    assert_eq!(
        built.get("params").and_then(|v| v.get("message")),
        Some(&json!("hello"))
    );
}

#[test]
// 必要测试
fn onebot_reply_payload_uses_private_target_for_direct_message() {
    let payload = json!({
        "message_type": "private",
        "user_id": "445566"
    });
    let built = build_onebot_v11_text_reply_payload(
        payload.as_object().expect("test payload should be object"),
        "pong",
    )
    .expect("private payload should build");

    assert_eq!(
        built.get("params").and_then(|v| v.get("message_type")),
        Some(&json!("private"))
    );
    assert_eq!(
        built.get("params").and_then(|v| v.get("user_id")),
        Some(&json!("445566"))
    );
}
