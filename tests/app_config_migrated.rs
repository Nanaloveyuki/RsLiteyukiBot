#[path = "../src/app_config.rs"]
mod app_config;
#[path = "../src/tui/mod.rs"]
mod tui;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use app_config::*;
use liteyukibot_core::AdapterConfig;

fn temp_path(name: &str, ext: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.push(format!("rsliteyuki-{name}-{nanos}.{ext}"));
    path
}

#[test]
fn write_default_config_if_missing_creates_yaml_template() {
    let path = temp_path("config-create", "yaml");
    let _ = std::fs::remove_file(&path);

    write_default_config_if_missing(&path).expect("config file should be created");
    let content = std::fs::read_to_string(&path).expect("config file should be readable");
    assert!(content.contains("rust:"));
    assert!(content.contains("adapters: []"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn validate_app_config_reports_invalid_values() {
    let mut duplicate = AdapterConfig::default();
    duplicate.id = "dup".to_string();

    let mut invalid = AdapterConfig::default();
    invalid.id = "dup".to_string();
    invalid.endpoint.url = "".to_string();

    let doc = AppConfigDoc {
        rust: Some(AppRustSection {
            runtime: None,
            log: None,
            adapters: Some(vec![duplicate, invalid]),
            tui: Some(TuiConfigSection {
                resume: Some(TuiResumeSection {
                    store_path: Some("   ".to_string()),
                    max_sessions: Some(0),
                    max_size_mib: Some(0),
                }),
            }),
        }),
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        onebot_v11: None,
    };

    let warnings = validate_app_config(&doc);
    assert!(warnings.iter().any(|w| w.contains("duplicated adapter id")));
    assert!(warnings.iter().any(|w| w.contains("invalid adapter")));
    assert!(warnings.iter().any(|w| w.contains("store_path")));
    assert!(warnings.iter().any(|w| w.contains("max_sessions")));
    assert!(warnings.iter().any(|w| w.contains("max_size_mib")));
}

#[test]
fn runtime_reload_warnings_detect_low_level_runtime_fields() {
    let doc = AppConfigDoc {
        rust: Some(AppRustSection {
            runtime: Some(RuntimeConfigSection {
                worker_count: Some(8),
                ingress_queue: None,
                worker_queue: None,
            }),
            log: None,
            adapters: None,
            tui: None,
        }),
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        onebot_v11: None,
    };

    let current = ReloadWarningState::from_doc(&doc);
    let warnings = runtime_reload_warnings(None, &current);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("hot switching may cause unpredictable behavior"))
    );
}

#[test]
fn runtime_reload_warnings_skip_when_sensitive_fields_unchanged() {
    let doc = AppConfigDoc {
        rust: Some(AppRustSection {
            runtime: Some(RuntimeConfigSection {
                worker_count: Some(8),
                ingress_queue: Some(1024),
                worker_queue: Some(256),
            }),
            log: Some(LogConfigSection {
                mode: Some("color".to_string()),
                level: Some("info".to_string()),
                timezone: Some("local".to_string()),
                timestamp_format: Some("custom".to_string()),
                timestamp_pattern: Some("%Y-%m-%d %H:%M:%S".to_string()),
            }),
            adapters: None,
            tui: None,
        }),
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        onebot_v11: None,
    };

    let state = ReloadWarningState::from_doc(&doc);
    let warnings = runtime_reload_warnings(Some(&state), &state);
    assert!(warnings.is_empty());
}

#[test]
fn connect_websocket_both_mode_generates_forward_and_reverse_adapters() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: Some(ConnectConfigSection {
            websocket: Some(WebSocketConnectSection {
                enabled: Some(true),
                mode: Some("both".to_string()),
                url: Some("ws://127.0.0.1:3000/ws".to_string()),
                host: Some("0.0.0.0".to_string()),
                port: Some(8080),
                path: Some("/ws".to_string()),
                headers: None,
                token: None,
                timeout_seconds: Some(30),
                queue_capacity: Some(256),
                max_payload_size: Some(1024 * 1024),
                max_connections: Some(100),
                inbound_topic: None,
                outbound_topic: None,
                forward: None,
                reverse: None,
            }),
            tcp_http: None,
            sse: None,
        }),
        tui: None,
        onebot_v11: None,
    };

    let adapters = load_adapter_configs(&doc).expect("connect adapters should parse");
    assert!(
        adapters
            .iter()
            .any(|adapter| adapter.id == "connect-ws-forward")
    );
    assert!(
        adapters
            .iter()
            .any(|adapter| adapter.id == "connect-ws-reverse")
    );
}

#[test]
fn connect_websocket_port_without_mode_defaults_to_reverse() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: Some(ConnectConfigSection {
            websocket: Some(WebSocketConnectSection {
                enabled: Some(true),
                mode: None,
                url: None,
                host: Some("0.0.0.0".to_string()),
                port: Some(8090),
                path: Some("/ws".to_string()),
                headers: None,
                token: None,
                timeout_seconds: Some(30),
                queue_capacity: None,
                max_payload_size: Some(1024 * 1024),
                max_connections: Some(100),
                inbound_topic: None,
                outbound_topic: None,
                forward: None,
                reverse: None,
            }),
            tcp_http: None,
            sse: None,
        }),
        tui: None,
        onebot_v11: None,
    };

    let adapters = load_adapter_configs(&doc).expect("connect adapters should parse");
    assert_eq!(adapters.len(), 1);
    assert_eq!(adapters[0].id, "connect-ws-reverse");
    assert_eq!(adapters[0].max_payload_size, Some(1024 * 1024));
    assert_eq!(adapters[0].max_connections, Some(100));
}

#[test]
fn resolve_help_whitelist_accepts_numeric_and_prefixed_entries() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        onebot_v11: Some(OnebotV11ConfigSection {
            whitelist: vec![
                OnebotWhitelistEntry::UInt(3541766758),
                OnebotWhitelistEntry::Text("group:699493240".to_string()),
                OnebotWhitelistEntry::Text("   ".to_string()),
            ],
        }),
    };

    let whitelist = resolve_help_whitelist(&doc);
    assert!(whitelist.contains("3541766758"));
    assert!(whitelist.contains("group:699493240"));
    assert!(!whitelist.contains(""));
}

#[test]
fn validate_app_config_warns_empty_onebot_whitelist_entry() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        onebot_v11: Some(OnebotV11ConfigSection {
            whitelist: vec![OnebotWhitelistEntry::Text("  ".to_string())],
        }),
    };
    let warnings = validate_app_config(&doc);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("onebot-v11.whitelist"))
    );
}
