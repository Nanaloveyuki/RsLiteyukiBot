use std::collections::HashMap;

use liteyukibot_core::{
    AdapterConfig, AdapterEndpoint, AdapterManager, AdapterPacket, AdapterRoute, AdapterTransport,
    SseEvent, SseParser, decode_sse_event, encode_sse_event, sink_from_fn,
};

#[test]
fn sse_encode_decode_roundtrip() {
    let event = SseEvent {
        event: Some("message".to_string()),
        id: Some("42".to_string()),
        retry: Some(3000),
        data: "hello\nworld".to_string(),
    };
    let encoded = encode_sse_event(&event);
    let decoded = decode_sse_event(&encoded).expect("sse event should decode");
    assert_eq!(decoded.event, Some("message".to_string()));
    assert_eq!(decoded.id, Some("42".to_string()));
    assert_eq!(decoded.retry, Some(3000));
    assert_eq!(decoded.data, "hello\nworld".to_string());
}

#[test]
fn sse_parser_handles_split_chunks() {
    let mut parser = SseParser::default();
    let first = "event: ping\ndata: one\n\nid: 2\ndata:";
    let second = " two\n\n";

    let part1 = parser.push_chunk(first);
    assert_eq!(part1.len(), 1);
    assert_eq!(part1[0].event.as_deref(), Some("ping"));
    assert_eq!(part1[0].data, "one");

    let part2 = parser.push_chunk(second);
    assert_eq!(part2.len(), 1);
    assert_eq!(part2[0].id.as_deref(), Some("2"));
    assert_eq!(part2[0].data, "two");
}

#[test]
fn adapter_config_validate_checks_required_fields() {
    let valid = AdapterConfig {
        id: "http-main".to_string(),
        enabled: true,
        transport: AdapterTransport::Http,
        endpoint: AdapterEndpoint {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            token: None,
            timeout_ms: 1000,
        },
        route: AdapterRoute::default(),
        queue_capacity: 16,
        max_payload_size: None,
        max_connections: None,
    };
    assert!(valid.validate().is_ok());

    let invalid = AdapterConfig {
        id: "".to_string(),
        ..valid
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn adapter_manager_register_and_get() {
    let manager = AdapterManager::new();
    let config = AdapterConfig {
        id: "ws-forward-1".to_string(),
        enabled: true,
        transport: AdapterTransport::WebSocketForward,
        endpoint: AdapterEndpoint {
            url: "ws://127.0.0.1:9910/ws".to_string(),
            headers: HashMap::new(),
            token: None,
            timeout_ms: 1000,
        },
        route: AdapterRoute::default(),
        queue_capacity: 32,
        max_payload_size: None,
        max_connections: None,
    };
    manager
        .register(config.clone())
        .expect("register should succeed");
    let got = manager.get("ws-forward-1").expect("config should exist");
    assert_eq!(got.id, config.id);
    assert_eq!(got.transport, AdapterTransport::WebSocketForward);
}

fn make_http_config(id: &str) -> AdapterConfig {
    AdapterConfig {
        id: id.to_string(),
        enabled: true,
        transport: AdapterTransport::Http,
        endpoint: AdapterEndpoint {
            url: "http://127.0.0.1:9919/api".to_string(),
            headers: HashMap::new(),
            token: None,
            timeout_ms: 1000,
        },
        route: AdapterRoute::default(),
        queue_capacity: 8,
        max_payload_size: None,
        max_connections: Some(4),
    }
}

#[test]
fn adapter_replace_configs_rejects_duplicates_without_mutating_existing_state() {
    let manager = AdapterManager::new();
    let old = make_http_config("keep-http");
    manager
        .register(old.clone())
        .expect("initial register should succeed");

    let mut dup_a = make_http_config("dup");
    dup_a.endpoint.url = "http://127.0.0.1:9920/one".to_string();
    let mut dup_b = make_http_config("dup");
    dup_b.endpoint.url = "http://127.0.0.1:9920/two".to_string();

    let result = manager.replace_configs(vec![dup_a, dup_b]);
    assert!(result.is_err(), "duplicate replace should fail");

    let kept = manager.get("keep-http").expect("existing config should remain");
    assert_eq!(kept.id, old.id);
    assert_eq!(kept.endpoint.url, old.endpoint.url);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adapter_start_and_shutdown_are_idempotent_under_concurrency() {
    let manager = AdapterManager::new();
    manager
        .register(make_http_config("http-concurrent"))
        .expect("register should succeed");

    let sink = sink_from_fn(|_packet| async {});
    let mut start_set = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let manager = manager.clone();
        let sink = sink.clone();
        start_set.spawn(async move { manager.start("http-concurrent", sink).await });
    }

    while let Some(joined) = start_set.join_next().await {
        joined
            .expect("start task should not panic")
            .expect("concurrent start should succeed");
    }

    assert!(manager.is_running("http-concurrent"));

    let mut shutdown_set = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let manager = manager.clone();
        shutdown_set.spawn(async move { manager.shutdown("http-concurrent").await });
    }

    while let Some(joined) = shutdown_set.join_next().await {
        joined
            .expect("shutdown task should not panic")
            .expect("concurrent shutdown should succeed");
    }

    assert!(!manager.is_running("http-concurrent"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adapter_shutdown_remains_effective_when_send_is_inflight() {
    let manager = AdapterManager::new();
    let mut reverse = AdapterConfig::default();
    reverse.id = "ws-race".to_string();
    reverse.transport = AdapterTransport::WebSocketReverse;
    reverse.endpoint = AdapterEndpoint {
        url: "ws://127.0.0.1:0/ws".to_string(),
        headers: HashMap::new(),
        token: None,
        timeout_ms: 500,
    };
    reverse.queue_capacity = 4;
    manager.register(reverse).expect("register should succeed");

    let sink = sink_from_fn(|_packet| async {});
    manager
        .start("ws-race", sink)
        .await
        .expect("reverse adapter should start");

    let manager_for_send = manager.clone();
    let send_task = tokio::spawn(async move {
        let packet = AdapterPacket::new(
            "test-packet",
            "onebot.v11.api.ping",
            serde_json::json!({ "action": "get_status" }),
        );
        let _ = manager_for_send.send("ws-race", packet).await;
    });

    tokio::task::yield_now().await;
    manager
        .shutdown("ws-race")
        .await
        .expect("shutdown should succeed");
    send_task
        .await
        .expect("send task should not panic even when adapter is shutting down");

    assert!(
        !manager.is_running("ws-race"),
        "shutdown should remain effective after concurrent send"
    );
}
