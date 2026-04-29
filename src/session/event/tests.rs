use super::*;
use crate::core::BotEvent;
use serde_json::json;

#[test]
// 必要测试
fn from_bot_event_extracts_onebot_v11_fields() {
    let event = BotEvent::new(
        1,
        "adapter.inbound",
        json!({
            "self_id": 42,
            "post_type": "message",
            "message_type": "group",
            "group_id": 123456,
            "user_id": 10001,
            "raw_message": "你好"
        }),
    );

    let session = SessionEvent::from_bot_event(&event);
    assert_eq!(session.bot_id.as_ref(), "42");
    assert_eq!(session.user_id.as_ref(), "10001");
    assert_eq!(session.session_id.as_ref(), "123456");
    assert_eq!(session.message.as_ref(), "你好");
    assert_eq!(session.scope, SessionScope::Group);
}

#[test]
// 必要测试
fn from_bot_event_joins_message_segments() {
    let event = BotEvent::new(
        2,
        "adapter.inbound",
        json!({
            "message_type": "private",
            "user_id": "u100",
            "message": [
                { "type": "text", "data": { "text": "hello" } },
                { "type": "text", "data": { "text": " world" } }
            ]
        }),
    );

    let session = SessionEvent::from_bot_event(&event);
    assert_eq!(session.message.as_ref(), "hello world");
    assert_eq!(session.scope, SessionScope::Private);
}
