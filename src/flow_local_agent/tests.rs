use super::client::FlowLocalAgentClient;
use super::protocol::{
    FlowLocalAgentClientMessage, FlowLocalAgentCloseCode, FlowLocalAgentConfirmEnvelope,
    FlowLocalAgentServerMessage,
};
use super::state::FlowLocalAgentRuntimeState;
use crate::app_config::FlowLocalAgentRuntimeConfig;
use crate::runtime_support::PreparedFlowLocalAgentRuntime;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn close_codes_keep_reconnect_policy_narrow() {
    assert!(!FlowLocalAgentCloseCode::should_reconnect(4001));
    assert!(!FlowLocalAgentCloseCode::should_reconnect(4002));
    assert!(!FlowLocalAgentCloseCode::should_reconnect(4003));
    assert!(FlowLocalAgentCloseCode::should_reconnect(1011));
}

#[test]
fn runtime_state_snapshot_tracks_connection_flags() {
    let state = FlowLocalAgentRuntimeState::default();
    assert_eq!(state.snapshot().connected, false);

    state.mark_connected();
    let connected = state.snapshot();
    assert!(connected.connected);
    assert!(connected.reconnect_allowed);
    assert!(connected.last_error.is_none());

    state.mark_disconnected(false, Some("invalid token".to_string()));
    let disconnected = state.snapshot();
    assert!(!disconnected.connected);
    assert!(!disconnected.reconnect_allowed);
    assert_eq!(disconnected.last_error.as_deref(), Some("invalid token"));
}

#[tokio::test]
async fn client_run_returns_placeholder_until_wired() {
    let client = FlowLocalAgentClient::new(FlowLocalAgentRuntimeState::default());
    let err = client
        .run()
        .await
        .expect_err("placeholder client should fail");
    assert!(err.contains("not wired yet"));
}

#[test]
fn protocol_messages_deserialize_expected_variants() {
    let ping: FlowLocalAgentServerMessage =
        serde_json::from_str(r#"{"type":"ping"}"#).expect("ping");
    assert!(matches!(ping, FlowLocalAgentServerMessage::Ping(_)));

    let confirm: FlowLocalAgentServerMessage = serde_json::from_value(serde_json::json!({
        "type": "confirm_response",
        "id": "req-1",
        "approved": true,
        "always": false
    }))
    .expect("confirm");
    assert!(matches!(
        confirm,
        FlowLocalAgentServerMessage::ConfirmResponse(FlowLocalAgentConfirmEnvelope {
            approved: true,
            ..
        })
    ));

    let pong = serde_json::to_string(&FlowLocalAgentClientMessage::Pong).expect("pong");
    assert_eq!(pong, r#"{"type":"pong"}"#);
}

#[test]
fn prepared_runtime_generates_and_reuses_persisted_device_id() {
    let _lock = process_state_lock();
    let path = temp_path("flow-device-id");
    let _ = fs::remove_file(&path);

    let _device_id_guard = EnvVarGuard::set("LY_FLOW_LOCAL_AGENT_DEVICE_ID_PATH", path.as_path());

    let (first, first_warnings) = PreparedFlowLocalAgentRuntime::new(runtime_config(None));
    let first_device_id = first
        .config_snapshot()
        .device_id
        .clone()
        .expect("device id should be generated");
    assert!(first_warnings.is_empty());

    let (second, second_warnings) = PreparedFlowLocalAgentRuntime::new(runtime_config(None));
    assert_eq!(
        second.config_snapshot().device_id.as_deref(),
        Some(first_device_id.as_str())
    );
    assert!(second_warnings.is_empty());

    let _ = fs::remove_file(path);
}

fn runtime_config(device_id: Option<String>) -> FlowLocalAgentRuntimeConfig {
    FlowLocalAgentRuntimeConfig {
        enabled: true,
        base_url: Some("https://flow.liteyuki.org".to_string()),
        token: Some("lys_test".to_string()),
        device_id,
        device_name: Some("Test Device".to_string()),
        auto_connect: true,
        allowed_tools: vec!["read_file".to_string()],
        workspace_root: None,
        command_timeout_ms: 30_000,
        approval_policy: "prompt".to_string(),
    }
}

fn temp_path(label: &str) -> PathBuf {
    let process_id = std::process::id();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "liteyuki-flow-local-agent-runtime-test-{label}-{process_id}-{unique}"
    ))
}
