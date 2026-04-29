use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use liteyukibot_core::recent_buffered_logs;

use super::llm::{LlmConfigPatch, describe_llm_patch};
use super::persist_disabled_commands;
use super::toml::{
    update_disabled_commands_document as update_toml_commands_document,
    update_disabled_plugins_document as update_toml_plugins_document,
    update_llm_document as update_toml_llm_document,
    update_whitelist_document as update_toml_document,
};
use super::yaml::{
    update_disabled_commands_document as update_yaml_commands_document,
    update_disabled_plugins_document as update_yaml_plugins_document,
    update_llm_document as update_yaml_llm_document,
    update_whitelist_document as update_yaml_document,
};
use crate::app_config::{LlmManagedModelConfig, LlmManagedProviderConfig};

fn temp_config_path(name: &str, ext: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("rsliteyukibot-{name}-{unique}.{ext}"))
}

#[test]
fn update_yaml_keeps_section_and_rewrites_whitelist() {
    let source = "onebot-v11:\n  whitelist: []\nrust:\n  adapters: []\n";
    let updated = update_yaml_document(source, &["private:1".to_string(), "group:2".to_string()]);
    assert!(updated.contains("onebot-v11:\n  whitelist:\n    - 'private:1'\n    - 'group:2'\n"));
    assert!(updated.contains("rust:\n  adapters: []"));
}

#[test]
fn update_yaml_inserts_section_when_missing() {
    let source = "rust:\n  adapters: []\n";
    let updated = update_yaml_document(source, &["private:42".to_string()]);
    assert!(updated.contains("onebot-v11:\n  whitelist:\n    - 'private:42'"));
}

#[test]
fn update_toml_rewrites_existing_table() {
    let source = "[onebot-v11]\nwhitelist = []\n\n[rust]\nadapters = []\n";
    let updated = update_toml_document(
        source,
        &["private:1000".to_string(), "group:2000".to_string()],
    );
    assert!(updated.contains("whitelist = [\"private:1000\", \"group:2000\"]"));
    assert!(updated.contains("[rust]"));
}

#[test]
fn update_yaml_commands_rewrites_disabled_entries() {
    let source = "commands:\n  disabled: []\nrust:\n  adapters: []\n";
    let updated = update_yaml_commands_document(
        source,
        &["adapter:onebot11 /su".to_string(), "tui /help".to_string()],
    );
    assert!(
        updated
            .contains("commands:\n  disabled:\n    - 'adapter:onebot11 /su'\n    - 'tui /help'\n")
    );
    assert!(updated.contains("rust:\n  adapters: []"));
}

#[test]
fn update_toml_commands_inserts_section_when_missing() {
    let source = "[rust]\nadapters = []\n";
    let updated = update_toml_commands_document(source, &["tui /help".to_string()]);
    assert!(updated.contains("[commands]"));
    assert!(updated.contains("disabled = [\"tui /help\"]"));
    assert!(updated.contains("[rust]"));
}

#[test]
fn update_yaml_plugins_rewrites_disabled_entries() {
    let source = "plugins:\n  disabled: []\nrust:\n  adapters: []\n";
    let updated = update_yaml_plugins_document(
        source,
        &["builtin-liteecho".to_string(), "demo-plugin".to_string()],
    );
    assert!(
        updated.contains("plugins:\n  disabled:\n    - 'builtin-liteecho'\n    - 'demo-plugin'\n")
    );
    assert!(updated.contains("rust:\n  adapters: []"));
}

#[test]
fn update_toml_plugins_inserts_section_when_missing() {
    let source = "[rust]\nadapters = []\n";
    let updated = update_toml_plugins_document(source, &["builtin-liteecho".to_string()]);
    assert!(updated.contains("[plugins]"));
    assert!(updated.contains("disabled = [\"builtin-liteecho\"]"));
    assert!(updated.contains("[rust]"));
}

#[test]
fn update_yaml_disabled_list_preserves_sibling_keys_inside_same_section() {
    let source = "commands:\n  disabled: []\n  prefix: '/'\nrust:\n  adapters: []\n";
    let updated = update_yaml_commands_document(source, &["tui /help".to_string()]);
    assert!(updated.contains("commands:\n  disabled:\n    - 'tui /help'\n  prefix: '/'"));
}

#[test]
fn update_toml_disabled_list_preserves_following_keys_inside_same_table() {
    let source = "[commands]\ndisabled = []\nprefix = \"/\"\n\n[rust]\nadapters = []\n";
    let updated = update_toml_commands_document(source, &["tui /help".to_string()]);
    assert!(updated.contains("[commands]\ndisabled = [\"tui /help\"]\nprefix = \"/\""));
    assert!(updated.contains("[rust]"));
}

#[test]
fn update_yaml_llm_rewrites_target_fields() {
    let source = "llm:\n  enabled: false\n  provider: 'openai'\n  model: 'gpt-old'\n";
    let patch = LlmConfigPatch {
        enabled: Some(true),
        provider: Some("openai".to_string()),
        base_url: Some("https://api.openai.com".to_string()),
        provider_urls: Some(vec![
            "https://api.openai.com".to_string(),
            "https://tokenflux.dev/v1".to_string(),
        ]),
        model: Some("gpt-4.1-mini".to_string()),
        api_keys: Some(vec!["k1".to_string(), "k2".to_string()]),
        ..Default::default()
    };
    let updated = update_yaml_llm_document(source, &patch);
    assert!(updated.contains("llm:"));
    assert!(updated.contains("enabled: true"));
    assert!(updated.contains("provider: 'openai'"));
    assert!(updated.contains("base_url: 'https://api.openai.com'"));
    assert!(updated.contains(
        "provider_urls:\n    - 'https://api.openai.com'\n    - 'https://tokenflux.dev/v1'"
    ));
    assert!(updated.contains("model: 'gpt-4.1-mini'"));
    assert!(updated.contains("api_keys:\n    - 'k1'\n    - 'k2'"));
}

#[test]
fn update_toml_llm_inserts_section_when_missing() {
    let source = "[rust]\nadapters = []\n";
    let patch = LlmConfigPatch {
        enabled: Some(true),
        provider: Some("openai".to_string()),
        base_url: Some("https://api.openai.com".to_string()),
        provider_urls: Some(vec!["https://api.openai.com".to_string()]),
        model: Some("gpt-4.1-mini".to_string()),
        api_keys: Some(vec!["k1".to_string()]),
        ..Default::default()
    };
    let updated = update_toml_llm_document(source, &patch);
    assert!(updated.contains("[llm]"));
    assert!(updated.contains("enabled = true"));
    assert!(updated.contains("provider = \"openai\""));
    assert!(updated.contains("base_url = \"https://api.openai.com\""));
    assert!(updated.contains("provider_urls = [\"https://api.openai.com\"]"));
    assert!(updated.contains("model = \"gpt-4.1-mini\""));
    assert!(updated.contains("api_keys = [\"k1\"]"));
}

#[test]
fn update_yaml_llm_allows_clearing_provider_urls() {
    let source = "llm:\n  base_url: 'https://api.openai.com'\n  provider_urls:\n    - 'https://api.openai.com'\n";
    let patch = LlmConfigPatch {
        provider_urls: Some(vec![]),
        ..Default::default()
    };
    let updated = update_yaml_llm_document(source, &patch);
    assert!(updated.contains("provider_urls: []"));
}

#[test]
fn update_yaml_llm_enabled_only_preserves_existing_provider_fields() {
    let source = "llm:\n  enabled: false\n  provider: 'openai'\n  model: 'gpt-5-mini'\n";
    let patch = LlmConfigPatch {
        enabled: Some(true),
        ..Default::default()
    };

    let updated = update_yaml_llm_document(source, &patch);

    assert!(updated.contains("enabled: true"));
    assert!(updated.contains("provider: 'openai'"));
    assert!(updated.contains("model: 'gpt-5-mini'"));
}

#[test]
fn update_yaml_llm_writes_managed_provider_blocks() {
    let source = "llm:\n  enabled: false\n";
    let mut headers = std::collections::HashMap::new();
    headers.insert("x-title".to_string(), "Liteyuki".to_string());
    let patch = LlmConfigPatch {
        timeout_seconds: Some(120),
        headers: Some(headers.clone()),
        active_provider_id: Some("grok".to_string()),
        providers: Some(vec![LlmManagedProviderConfig {
            id: Some("grok".to_string()),
            label: Some("grok".to_string()),
            provider: Some("openai-compatible".to_string()),
            base_url: Some("https://tokenflux.dev/v1".to_string()),
            api_key: Some("sk-demo".to_string()),
            timeout_seconds: Some(120),
            headers: Some(headers),
            models: Some(vec![LlmManagedModelConfig {
                id: Some("grok-4".to_string()),
                enabled: Some(true),
            }]),
        }]),
        ..Default::default()
    };

    let updated = update_yaml_llm_document(source, &patch);
    assert!(updated.contains("timeout_seconds: 120"));
    assert!(updated.contains("active_provider_id: 'grok'"));
    assert!(updated.contains("headers:\n    'x-title': 'Liteyuki'"));
    assert!(updated.contains("providers:"));
    assert!(updated.contains("- id: 'grok'"));
    assert!(updated.contains("provider: 'openai-compatible'"));
    assert!(updated.contains("models:"));
    assert!(updated.contains("- id: 'grok-4'"));
    assert!(updated.contains("enabled: true"));
}

#[test]
fn update_toml_llm_writes_managed_provider_blocks() {
    let source = "[llm]\nenabled = false\n";
    let mut headers = std::collections::HashMap::new();
    headers.insert("x-title".to_string(), "Liteyuki".to_string());
    let patch = LlmConfigPatch {
        timeout_seconds: Some(120),
        headers: Some(headers.clone()),
        active_provider_id: Some("grok".to_string()),
        providers: Some(vec![LlmManagedProviderConfig {
            id: Some("grok".to_string()),
            label: Some("grok".to_string()),
            provider: Some("openai-compatible".to_string()),
            base_url: Some("https://tokenflux.dev/v1".to_string()),
            api_key: Some("sk-demo".to_string()),
            timeout_seconds: Some(120),
            headers: Some(headers),
            models: Some(vec![LlmManagedModelConfig {
                id: Some("grok-4".to_string()),
                enabled: Some(true),
            }]),
        }]),
        ..Default::default()
    };

    let updated = update_toml_llm_document(source, &patch);
    assert!(updated.contains("timeout_seconds = 120"));
    assert!(updated.contains("active_provider_id = \"grok\""));
    assert!(updated.contains("headers = { \"x-title\" = \"Liteyuki\" }"));
    assert!(updated.contains("[[llm.providers]]"));
    assert!(updated.contains("provider = \"openai-compatible\""));
    assert!(updated.contains("models = [{ id = \"grok-4\", enabled = true }]"));
}

#[test]
fn describe_llm_patch_redacts_api_key_values() {
    let patch = LlmConfigPatch {
        provider: Some("openai".to_string()),
        api_keys: Some(vec!["sk-secret-1".to_string(), "sk-secret-2".to_string()]),
        ..Default::default()
    };

    let summary = describe_llm_patch(&patch);

    assert!(summary.contains("provider=openai"));
    assert!(summary.contains("api_keys=<updated:2>"));
    assert!(!summary.contains("sk-secret-1"));
    assert!(!summary.contains("sk-secret-2"));
}

#[test]
fn persist_disabled_commands_emits_buffered_log_entry() {
    let path = temp_config_path("disabled-commands-log", "yaml");
    fs::write(&path, "commands:\n  disabled: []\n").expect("test config should be written");

    persist_disabled_commands(&path, &["tui /help".to_string()]).expect("persist should succeed");

    let path_display = path.display().to_string();
    assert!(recent_buffered_logs(50).iter().any(|entry| {
        entry.module == "config.edit"
            && entry.message.contains("persisted disabled commands")
            && entry.message.contains(path_display.as_str())
            && entry.message.contains("entries=1")
    }));

    let _ = fs::remove_file(path);
}
