use std::path::Path;

#[path = "config_edit/llm.rs"]
mod llm;
#[path = "config_edit/shared.rs"]
mod shared;
#[cfg(test)]
#[path = "config_edit/tests.rs"]
mod tests;
#[path = "config_edit/toml.rs"]
mod toml;
#[path = "config_edit/yaml.rs"]
mod yaml;

pub use llm::LlmConfigPatch;

use llm::{describe_llm_patch, normalize_llm_patch};
use shared::persist_config_with_format;

// 外部调用
#[allow(dead_code)]
pub fn write_text_file_atomically(path: &Path, content: &str) -> Result<(), String> {
    shared::write_text_file_atomically(path, content)
}

pub fn persist_onebot_v11_whitelist(path: &Path, entries: &[String]) -> Result<(), String> {
    let normalized = shared::normalize_entries(entries);
    let detail = format!("entries={}", normalized.len());
    persist_config_with_format(
        "onebot-v11 whitelist",
        path,
        detail.as_str(),
        |content| yaml::update_whitelist_document(content, &normalized),
        |content| toml::update_whitelist_document(content, &normalized),
    )
}

pub fn persist_disabled_commands(path: &Path, entries: &[String]) -> Result<(), String> {
    let normalized = shared::normalize_entries(entries);
    let detail = format!("entries={}", normalized.len());
    persist_config_with_format(
        "disabled commands",
        path,
        detail.as_str(),
        |content| yaml::update_disabled_commands_document(content, &normalized),
        |content| toml::update_disabled_commands_document(content, &normalized),
    )
}

pub fn persist_disabled_plugins(path: &Path, entries: &[String]) -> Result<(), String> {
    let normalized = shared::normalize_entries(entries);
    let detail = format!("entries={}", normalized.len());
    persist_config_with_format(
        "disabled plugins",
        path,
        detail.as_str(),
        |content| yaml::update_disabled_plugins_document(content, &normalized),
        |content| toml::update_disabled_plugins_document(content, &normalized),
    )
}

#[allow(dead_code)] // 外部调用
pub fn persist_desktop_close_to_tray(path: &Path, close_to_tray: bool) -> Result<(), String> {
    let detail = format!("close_to_tray={close_to_tray}");
    persist_config_with_format(
        "desktop close behavior",
        path,
        detail.as_str(),
        |content| {
            yaml::update_bool_section_document(content, "desktop", "close_to_tray", close_to_tray)
        },
        |content| {
            toml::update_bool_section_document(content, "desktop", "close_to_tray", close_to_tray)
        },
    )
}

pub fn persist_llm_config(path: &Path, patch: &LlmConfigPatch) -> Result<(), String> {
    let normalized_patch = normalize_llm_patch(patch);
    let detail = describe_llm_patch(&normalized_patch);
    persist_config_with_format(
        "llm config",
        path,
        detail.as_str(),
        |content| yaml::update_llm_document(content, &normalized_patch),
        |content| toml::update_llm_document(content, &normalized_patch),
    )
}
