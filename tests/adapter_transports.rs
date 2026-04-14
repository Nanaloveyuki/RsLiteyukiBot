use std::collections::HashMap;

use liteyukibot_core::{
    AdapterConfig, AdapterEndpoint, AdapterManager, AdapterRoute, AdapterTransport, SseEvent,
    SseParser, decode_sse_event, encode_sse_event,
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
