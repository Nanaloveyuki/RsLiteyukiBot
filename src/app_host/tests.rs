use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::RuntimeTarget;
use crate::app_config::LlmConfigSection;
use crate::runtime_support::{
    dedup_warnings, merge_llm_config_sections, push_explicit_plugin_dir_candidates,
    push_runtime_plugin_dir_candidates,
};
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};

use super::resource_usage::{normalize_percent, usage_percent};
use super::*;

fn temp_path(name: &str, ext: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("rsliteyukibot-{name}-{unique}.{ext}"))
}

#[test]
fn plugin_dir_candidates_cover_runtime_and_dev_layouts() {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let root = PathBuf::from("C:/liteyuki");
    push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, root.as_path(), true);

    assert!(dirs.contains(&root.join("builtin_plugin")));
    assert!(dirs.contains(&root.join("resources").join("builtin_plugin")));
    assert!(dirs.contains(&root.join("src").join("builtin_plugin")));
}

#[test]
fn explicit_plugin_paths_support_directories_and_install_roots() {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let root = PathBuf::from("C:/liteyuki");
    push_explicit_plugin_dir_candidates(&mut dirs, &mut seen, root.as_path());

    assert!(dirs.contains(&root));
    assert!(dirs.contains(&root.join("builtin_plugin")));
    assert!(dirs.contains(&root.join("resources").join("builtin_plugin")));
    assert!(dirs.contains(&root.join("src").join("builtin_plugin")));
}

#[test]
fn merge_llm_config_sections_prefers_overlay_values() {
    let merged = merge_llm_config_sections(
        Some(LlmConfigSection {
            stream: Some(false),
            provider: Some("openai".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            temperature: Some(0.6),
            top_p: Some(0.9),
            top_k: Some(16),
            parallel_tool_calls: Some(true),
            command_prefix: Some("/ask".to_string()),
            ..Default::default()
        }),
        LlmConfigSection {
            stream: Some(true),
            model: Some("gpt-4.1".to_string()),
            temperature: Some(0.2),
            top_p: Some(0.8),
            top_k: Some(32),
            parallel_tool_calls: Some(false),
            command_prefix: Some("/qa".to_string()),
            ..Default::default()
        },
    );

    assert_eq!(merged.stream, Some(true));
    assert_eq!(merged.provider.as_deref(), Some("openai"));
    assert_eq!(merged.model.as_deref(), Some("gpt-4.1"));
    assert_eq!(merged.temperature, Some(0.2));
    assert_eq!(merged.top_p, Some(0.8));
    assert_eq!(merged.top_k, Some(32));
    assert_eq!(merged.parallel_tool_calls, Some(false));
    assert_eq!(merged.command_prefix.as_deref(), Some("/qa"));
}

#[test]
fn dedup_warnings_preserves_first_occurrence() {
    let warnings = dedup_warnings(vec![
        "a".to_string(),
        "b".to_string(),
        "a".to_string(),
        "c".to_string(),
    ]);

    assert_eq!(warnings, vec!["a", "b", "c"]);
}

#[test]
fn usage_percent_handles_zero_total_and_clamps() {
    assert_eq!(usage_percent(10, 0), 0.0);
    assert_eq!(usage_percent(25, 100), 25.0);
    assert_eq!(usage_percent(150, 100), 100.0);
}

#[test]
fn normalize_percent_handles_invalid_values() {
    assert_eq!(normalize_percent(f32::NAN), 0.0);
    assert_eq!(normalize_percent(-4.0), 0.0);
    assert_eq!(normalize_percent(18.5), 18.5);
    assert_eq!(normalize_percent(180.0), 100.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_host_tolerates_adapter_autostart_failures() {
    let _lock = process_state_lock();
    let config_path = temp_path("embedded-host-config", "yaml");
    let llm_config_path = temp_path("embedded-host-llm", "yaml");
    let password_path = temp_path("embedded-host-password", "yaml");
    let config_source = r#"
adapters:
  - id: sse-broken
    enabled: true
    transport: sse
    endpoint:
      url: http://127.0.0.1:1/sse
      timeout_ms: 100
    route:
      inbound_topic: adapter.inbound
      outbound_topic: adapter.outbound
    queue_capacity: 4
"#;
    fs::write(&config_path, config_source).expect("test config should be written");
    let _config_guard = EnvVarGuard::set("LY_CONFIG_PATH", config_path.as_path());
    let _llm_guard = EnvVarGuard::set("LY_LLM_CONFIG_PATH", llm_config_path.as_path());
    let _password_guard = EnvVarGuard::set("LY_PASSWORD_PATH", password_path.as_path());

    let host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
        .await
        .expect("embedded host should keep running when adapters fail");
    let snapshot = host.snapshot();

    assert_eq!(snapshot.status, "running");
    assert_eq!(snapshot.adapter_count, 1);
    assert!(snapshot.adapter_autostart);
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("embedded adapter autostart failed")),
        "expected embedded adapter warning, got {:?}",
        snapshot.warnings
    );
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("adapter sse error")),
        "expected adapter error detail, got {:?}",
        snapshot.warnings
    );

    host.shutdown()
        .await
        .expect("embedded host should shutdown cleanly");

    let _ = fs::remove_file(config_path);
    let _ = fs::remove_file(llm_config_path);
    let _ = fs::remove_file(password_path);
}
