use liteyukibot_core::{AdapterConfig, AdapterTransport};

#[test]
fn adapter_transport_accepts_websocket_aliases() {
    let forward: AdapterTransport =
        serde_json::from_str("\"websocket_forward\"").expect("forward alias should parse");
    assert_eq!(forward, AdapterTransport::WebSocketForward);

    let reverse: AdapterTransport =
        serde_json::from_str("\"websocket_reverse\"").expect("reverse alias should parse");
    assert_eq!(reverse, AdapterTransport::WebSocketReverse);
}

#[test]
fn adapter_config_validate_rejects_zero_limits() {
    let mut config = AdapterConfig::default();
    config.max_payload_size = Some(0);
    assert!(config.validate().is_err());

    config.max_payload_size = Some(1024);
    config.max_connections = Some(0);
    assert!(config.validate().is_err());
}
