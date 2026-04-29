use super::*;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("rsliteyukibot-web-ui-{name}-{unique}"))
}

#[test]
// 必要测试
fn resolve_frontend_dist_dir_uses_first_candidate_with_index_html() {
    let missing_root = temp_dir_path("missing");
    let valid_root = temp_dir_path("valid");

    fs::create_dir_all(&missing_root).expect("missing candidate dir should exist");
    fs::create_dir_all(&valid_root).expect("valid candidate dir should exist");
    fs::write(valid_root.join("index.html"), "<!doctype html>")
        .expect("index.html should be written");

    let resolved = resolve_frontend_dist_dir_from_candidates([
        Some(missing_root.clone()),
        Some(valid_root.clone()),
    ]);

    assert_eq!(resolved, Some(normalize_path(valid_root.clone())));

    let _ = fs::remove_file(valid_root.join("index.html"));
    let _ = fs::remove_dir(&missing_root);
    let _ = fs::remove_dir(&valid_root);
}

#[test]
// 必要测试
fn resolve_frontend_dist_dir_returns_none_without_index_html() {
    let missing_root = temp_dir_path("none");
    fs::create_dir_all(&missing_root).expect("candidate dir should exist");

    let resolved = resolve_frontend_dist_dir_from_candidates([Some(missing_root.clone())]);

    assert!(resolved.is_none());

    let _ = fs::remove_dir(&missing_root);
}

#[test]
// 必要测试
fn resolve_dev_frontend_reads_probe_addr_from_env() {
    let _lock = process_state_lock();
    let _env_guard = EnvVarGuard::set(WEB_DEV_SERVER_ENV, "127.0.0.1:1420");

    let dev_frontend = resolve_dev_frontend_from_env();

    assert_eq!(
        dev_frontend,
        Some(WebHostDevServer {
            probe_addr: "127.0.0.1:1420".parse().expect("socket addr should parse"),
            public_port: 1420,
        })
    );
}

#[test]
// 必要测试
fn onebot_config_defaults_match_napcat_dashboard_shape() {
    let config = OneBotConfig::default();
    let json = serde_json::to_value(&config).expect("config should serialize");

    assert_eq!(json["network"]["httpServers"], serde_json::json!([]));
    assert_eq!(json["network"]["httpClients"], serde_json::json!([]));
    assert_eq!(json["network"]["httpSseServers"], serde_json::json!([]));
    assert_eq!(json["network"]["websocketServers"], serde_json::json!([]));
    assert_eq!(json["network"]["websocketClients"], serde_json::json!([]));
    assert_eq!(json["parseMultMsg"], true);
    assert_eq!(
        json["timeout"]["baseTimeout"],
        DEFAULT_FILE_TRANSFER_TIMEOUT_MS
    );
    assert_eq!(
        json["timeout"]["uploadSpeedKBps"],
        DEFAULT_FILE_TRANSFER_SPEED_KBPS
    );
}
