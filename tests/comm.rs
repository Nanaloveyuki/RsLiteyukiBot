use std::time::Duration;

use serde_json::json;

use liteyukibot_core::{ChannelMessage, ChannelRegistry, SharedStore};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_reuses_channel_instances() {
    let registry = ChannelRegistry::new();
    let channel_a = registry.get_or_create("shared-topic", 4);
    let channel_b = registry.get_or_create("shared-topic", 6);

    let mut receiver = channel_b.subscribe();
    let message = ChannelMessage::try_new(
        "shared-topic",
        json!({ "value": "payload" }),
        Some("source"),
    )
    .expect("serialize payload");

    channel_a.send(message.clone()).expect("send succeeded");

    let received = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .expect("should not timeout")
        .expect("message available");

    assert_eq!(received.topic, message.topic);
    assert_eq!(received.payload, message.payload);
    assert_eq!(received.source, message.source);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_store_can_publish_and_subscribe() {
    let registry = ChannelRegistry::new();
    let store = SharedStore::new(registry.clone());
    let mut receiver = store.subscribe("pubsub-topic");

    let message =
        ChannelMessage::try_new("pubsub-topic", json!({ "event": "ping" }), Some("store")).unwrap();

    store
        .publish("pubsub-topic", message.clone())
        .expect("publish succeeded");

    let received = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .expect("should not timeout")
        .expect("message available");

    assert_eq!(received.topic, message.topic);
    assert_eq!(received.payload, message.payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_store_set_get_delete_snapshot() {
    let registry = ChannelRegistry::new();
    let store = SharedStore::new(registry);

    store.set("alpha", json!(1));
    store.set("beta", json!([1, 2, 3]));

    assert_eq!(store.get("alpha"), Some(json!(1)));
    assert_eq!(store.get("beta"), Some(json!([1, 2, 3])));
    assert_eq!(store.delete("alpha"), Some(json!(1)));
    assert!(store.get("alpha").is_none());

    let snapshot = store.snapshot();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot["beta"], json!([1, 2, 3]));
}
