#[path = "../src/onebot_support.rs"]
mod onebot_support;

use std::collections::HashSet;
use std::sync::Arc;

use liteyukibot_core::core::BotEvent;
use liteyukibot_core::session::SessionEvent;
use serde_json::Value;

use onebot_support::*;

#[test]
fn onebot_heartbeat_can_be_hidden_from_tui() {
    let event = BotEvent::new(
        1,
        "adapter.inbound",
        serde_json::json!({
            "post_type": "meta_event",
            "meta_event_type": "heartbeat",
            "self_id": 1234
        }),
    );
    assert!(should_hide_event_from_tui(&event));
}

#[test]
fn build_help_reply_payload_for_group_message() {
    let event = SessionEvent {
        event_id: 99,
        topic: Arc::from("adapter.inbound"),
        message: Arc::from("/help"),
        payload: serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 114514,
            "user_id": 1919
        }),
        timestamp_ms: 0,
        bot_id: Arc::from("1"),
        session_id: Arc::from("114514"),
        user_id: Arc::from("1919"),
        scope: liteyukibot_core::SessionScope::Group,
    };

    let payload =
        build_onebot_v11_help_reply_payload(&event, "test-echo").expect("should build payload");
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("send_msg")
    );
    assert_eq!(
        payload.get("echo").and_then(Value::as_str),
        Some("test-echo")
    );
    assert_eq!(
        payload
            .get("params")
            .and_then(Value::as_object)
            .and_then(|params| params.get("group_id")),
        Some(&serde_json::json!(114514))
    );
}

#[test]
fn help_command_matcher_supports_help_variants() {
    assert!(is_help_command("/help"));
    assert!(is_help_command("help"));
    assert!(!is_help_command("/log"));
}

#[test]
fn help_session_whitelist_matches_private_and_group_rules() {
    let private_event = SessionEvent {
        event_id: 1,
        topic: Arc::from("adapter.inbound"),
        message: Arc::from("/help"),
        payload: serde_json::json!({}),
        timestamp_ms: 0,
        bot_id: Arc::from("1"),
        session_id: Arc::from("3541766758"),
        user_id: Arc::from("3541766758"),
        scope: liteyukibot_core::SessionScope::Private,
    };
    let group_event = SessionEvent {
        event_id: 2,
        topic: Arc::from("adapter.inbound"),
        message: Arc::from("/help"),
        payload: serde_json::json!({}),
        timestamp_ms: 0,
        bot_id: Arc::from("1"),
        session_id: Arc::from("699493240"),
        user_id: Arc::from("3541766758"),
        scope: liteyukibot_core::SessionScope::Group,
    };

    let whitelist: HashSet<String> = ["private:3541766758", "group:699493240"]
        .iter()
        .map(|entry| entry.to_string())
        .collect();
    assert!(is_help_session_allowed(&private_event, &whitelist));
    assert!(is_help_session_allowed(&group_event, &whitelist));

    let private_only: HashSet<String> = ["3541766758"].iter().map(|e| e.to_string()).collect();
    assert!(is_help_session_allowed(&private_event, &private_only));
    assert!(!is_help_session_allowed(&group_event, &private_only));
}

#[test]
fn private_prefix_match_is_tolerant_to_scope_drift() {
    let event = SessionEvent {
        event_id: 7,
        topic: Arc::from("adapter.inbound"),
        message: Arc::from("/help"),
        payload: serde_json::json!({
            "message_type": "private",
            "user_id": "3541766758"
        }),
        timestamp_ms: 0,
        bot_id: Arc::from("1"),
        session_id: Arc::from("3541766758"),
        user_id: Arc::from("3541766758"),
        scope: liteyukibot_core::SessionScope::Other(Arc::from("unknown")),
    };

    let whitelist: HashSet<String> = ["private:3541766758"]
        .iter()
        .map(|entry| entry.to_string())
        .collect();
    assert!(is_help_session_allowed(&event, &whitelist));
}

#[test]
fn private_prefix_does_not_match_group_message_with_same_user_id() {
    let event = SessionEvent {
        event_id: 8,
        topic: Arc::from("adapter.inbound"),
        message: Arc::from("/help"),
        payload: serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": "758234884",
            "user_id": "3541766758"
        }),
        timestamp_ms: 0,
        bot_id: Arc::from("1"),
        session_id: Arc::from("758234884"),
        user_id: Arc::from("3541766758"),
        scope: liteyukibot_core::SessionScope::Group,
    };

    let whitelist: HashSet<String> = ["private:3541766758"]
        .iter()
        .map(|entry| entry.to_string())
        .collect();
    assert!(!is_help_session_allowed(&event, &whitelist));
}

#[test]
fn render_group_preview_for_cq_image_without_summary_mode() {
    let payload = serde_json::json!({
        "post_type": "message",
        "message_type": "group",
        "group_id": 206017422,
        "user_id": 3555801168u64,
        "raw_message": "[CQ:image,summary=&#91;动画表情&#93;,file=abc.jpg]"
    });

    let object = payload.as_object().expect("payload should be object");
    let preview = render_onebot_message_preview(object, false);
    assert_eq!(preview, "群聊[206017422:3555801168] [图片]");
}

#[test]
fn render_private_preview_for_cq_image_with_summary_mode() {
    let payload = serde_json::json!({
        "post_type": "message",
        "message_type": "private",
        "user_id": 3541766758u64,
        "raw_message": "[CQ:image,summary=&#91;动画表情&#93;,file=abc.jpg]"
    });

    let object = payload.as_object().expect("payload should be object");
    let preview = render_onebot_message_preview(object, true);
    assert_eq!(preview, "私聊[3541766758] [动画表情]");
}
