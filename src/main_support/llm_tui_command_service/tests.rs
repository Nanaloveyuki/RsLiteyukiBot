use super::*;
use crate::external_commands::{llm_usage_text, matches_external_ask_command};
use crate::runtime_support::LlmCommandRuntime;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_llm_config_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("rsliteyukibot-{name}-{unique}.yaml"))
}

fn run_llm_command_for_test(action: tui::LlmCommandRequest) -> Result<String, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build")
        .block_on(handle_llm_tui_command(action))
}

#[test]
// 必要测试
fn llm_provider_use_rejects_unregistered_base_url_without_mutating_config() {
    let _lock = process_state_lock();
    let path = temp_llm_config_path("provider-use-invalid");
    let source = "llm:\n  base_url: https://api.openai.com\n  provider_urls:\n    - https://api.openai.com\n    - https://tokenflux.dev/v1\n";
    fs::write(&path, source).expect("test llm config should be written");
    let _env_guard = EnvVarGuard::set("LY_LLM_CONFIG_PATH", path.as_path());

    let result = run_llm_command_for_test(tui::LlmCommandRequest::UseProviderUrl(
        "https://typo.example/v1".to_string(),
    ))
    .expect_err("unregistered provider url should be rejected");

    assert!(result.contains("https://typo.example/v1"));
    let updated = fs::read_to_string(&path).expect("test llm config should remain readable");
    assert_eq!(updated, source);

    let _ = fs::remove_file(path);
}

#[test]
// 必要测试
fn llm_provider_use_switches_to_registered_base_url() {
    let _lock = process_state_lock();
    let path = temp_llm_config_path("provider-use-valid");
    let source = "llm:\n  base_url: https://api.openai.com\n  provider_urls:\n    - https://api.openai.com\n    - https://tokenflux.dev/v1\n";
    fs::write(&path, source).expect("test llm config should be written");
    let _env_guard = EnvVarGuard::set("LY_LLM_CONFIG_PATH", path.as_path());

    let result = run_llm_command_for_test(tui::LlmCommandRequest::UseProviderUrl(
        "https://tokenflux.dev/v1".to_string(),
    ))
    .expect("registered provider url should switch successfully");

    assert!(result.contains("https://tokenflux.dev/v1"));
    assert!(result.contains("count=2"));
    assert!(result.contains(path.display().to_string().as_str()));
    let updated = fs::read_to_string(&path).expect("updated llm config should be readable");
    assert!(updated.contains("base_url: 'https://tokenflux.dev/v1'"));
    assert!(updated.contains("provider_urls:"));
    assert!(updated.contains("- 'https://api.openai.com'"));
    assert!(updated.contains("- 'https://tokenflux.dev/v1'"));

    let _ = fs::remove_file(path);
}

#[test]
// 必要测试
fn external_ask_command_prefix_reads_runtime_updates() {
    let llm_runtime = LlmCommandRuntime::new("/ask");

    assert!(matches_external_ask_command(
        "/ask hello world",
        &llm_runtime
    ));
    assert_eq!(
        llm_usage_text(llm_runtime.command_prefix().as_str()),
        "用法: /ask 你的问题"
    );

    llm_runtime.set_command_prefix("/qa");

    assert!(!matches_external_ask_command(
        "/ask hello world",
        &llm_runtime
    ));
    assert!(matches_external_ask_command(
        "/qa hello world",
        &llm_runtime
    ));
    assert_eq!(
        llm_usage_text(llm_runtime.command_prefix().as_str()),
        "用法: /qa 你的问题"
    );
}
