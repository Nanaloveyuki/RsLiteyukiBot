use serde_json::json;

use super::*;

#[test]
// 必要测试
fn normalize_inbound_packet_rewrites_onebot_topic_and_injects_metadata() {
    let source_topic = "onebot.v11.event.message.group";
    let packet = AdapterPacket::new(
        "evt-1",
        source_topic,
        json!({
            "post_type": "message",
            "message_type": "group"
        }),
    );

    let normalized = normalize_inbound_packet(packet, "adapter.inbound", "ws-main");
    assert_eq!(normalized.topic, "adapter.inbound");
    assert_eq!(
        normalized
            .payload
            .get("_adapter_id")
            .and_then(Value::as_str),
        Some("ws-main")
    );
    assert_eq!(
        normalized
            .payload
            .get("_adapter_topic")
            .and_then(Value::as_str),
        Some(source_topic)
    );
    assert_eq!(
        normalized
            .payload
            .get("_adapter_ingress")
            .and_then(Value::as_str),
        Some("adapter.inbound")
    );
}

#[test]
// 必要测试
fn normalize_inbound_packet_keeps_existing_adapter_id_for_object_payload() {
    let packet = AdapterPacket::new(
        "evt-2",
        "custom.topic",
        json!({
            "_adapter_id": "upstream",
            "foo": "bar"
        }),
    );

    let normalized = normalize_inbound_packet(packet, "adapter.inbound", "local");
    assert_eq!(normalized.topic, "custom.topic");
    assert_eq!(
        normalized
            .payload
            .get("_adapter_id")
            .and_then(Value::as_str),
        Some("upstream")
    );
    assert!(
        normalized.payload.get("_adapter_topic").is_none(),
        "non-onebot topic should not add adapter_topic for object payload"
    );
}

#[test]
// 必要测试
fn normalize_inbound_packet_wraps_non_object_payload() {
    let packet = AdapterPacket::new("evt-3", "onebot.v11.event.notice", json!("raw-data"));

    let normalized = normalize_inbound_packet(packet, "adapter.inbound", "sse-main");
    assert_eq!(normalized.topic, "adapter.inbound");
    assert_eq!(
        normalized
            .payload
            .get("_adapter_id")
            .and_then(Value::as_str),
        Some("sse-main")
    );
    assert_eq!(
        normalized
            .payload
            .get("_adapter_topic")
            .and_then(Value::as_str),
        Some("onebot.v11.event.notice")
    );
    assert_eq!(normalized.payload.get("data"), Some(&json!("raw-data")));
}

#[tokio::test(flavor = "current_thread")]
// 必要测试
async fn websocket_pool_sender_uses_round_robin_distribution() {
    let (tx0, mut rx0) = tokio::sync::mpsc::channel::<AdapterPacket>(4);
    let (tx1, mut rx1) = tokio::sync::mpsc::channel::<AdapterPacket>(4);
    let pool = WebSocketPoolSender::new(vec![
        WebSocketOutboundSender::Forward(tx0),
        WebSocketOutboundSender::Forward(tx1),
    ])
    .expect("pool should build");

    pool.send(AdapterPacket::new("1", "adapter.outbound", json!({})))
        .await
        .expect("first send should succeed");
    pool.send(AdapterPacket::new("2", "adapter.outbound", json!({})))
        .await
        .expect("second send should succeed");

    let first = rx0.recv().await.expect("first lane should receive packet");
    let second = rx1.recv().await.expect("second lane should receive packet");
    assert_eq!(first.id, "1");
    assert_eq!(second.id, "2");
}

#[test]
// 必要测试
fn adapter_manager_parallelism_can_be_configured() {
    let manager = AdapterManager::new();
    assert_eq!(manager.parallelism(), 1);

    manager.set_parallelism(4);
    assert_eq!(manager.parallelism(), 4);
}

#[test]
// 必要测试
fn reverse_ws_max_connections_defaults_to_single_connection() {
    assert_eq!(resolve_reverse_ws_max_connections(None), Some(1));
    assert_eq!(resolve_reverse_ws_max_connections(Some(3)), Some(3));
}
