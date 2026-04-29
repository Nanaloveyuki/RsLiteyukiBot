use super::*;
use serde_json::json;

#[test]
// 必要测试
fn parse_packet_accepts_onebot_v11_event_json() {
    let message = Message::Text(
        r#"{"time":1710000000,"self_id":1234,"post_type":"message","message_type":"group","group_id":9876,"user_id":1001,"raw_message":"hello"}"#.to_string(),
    );

    let packet = parse_packet(message)
        .expect("packet should parse")
        .expect("message should decode");

    assert_eq!(packet.topic, "onebot.v11.event.message.group");
    assert_eq!(
        packet
            .payload
            .get("_adapter_protocol")
            .and_then(Value::as_str),
        Some("onebot.v11")
    );
    assert_eq!(packet.payload.get("raw_message"), Some(&json!("hello")));
}

#[test]
// 必要测试
fn serialize_packet_prefers_onebot_action_payload() {
    let packet = AdapterPacket::new(
        "1",
        "adapter.outbound",
        json!({
            "action": "send_group_msg",
            "params": { "group_id": 10000, "message": "hello" },
            "echo": "abc-1"
        }),
    );

    let encoded = serialize_packet(&packet).expect("encode should succeed");
    let parsed: Value = serde_json::from_str(&encoded).expect("encoded json should parse");

    assert_eq!(
        parsed.get("action").and_then(Value::as_str),
        Some("send_group_msg")
    );
    assert!(parsed.get("topic").is_none());
}
