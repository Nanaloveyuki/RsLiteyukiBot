use std::collections::HashMap;

use crate::app_config::{LlmManagedModelConfig, LlmManagedProviderConfig};

use super::flow_local_agent::FlowLocalAgentConfigPatch;
use super::llm::LlmConfigPatch;
use super::shared::{append_blank_line_if_needed, detect_newline, join_lines};

pub(crate) fn update_whitelist_document(content: &str, entries: &[String]) -> String {
    let mut document = TomlDocument::new(content);
    let table_start = document.ensure_named_table(&["onebot-v11", "onebot_v11"], "onebot-v11");
    upsert_table_key(
        &mut document.lines,
        table_start,
        "whitelist",
        render_toml_string_array(entries),
    );
    document.finish()
}

pub(crate) fn update_disabled_commands_document(content: &str, entries: &[String]) -> String {
    update_disabled_list_section(content, "commands", entries)
}

pub(crate) fn update_disabled_plugins_document(content: &str, entries: &[String]) -> String {
    update_disabled_list_section(content, "plugins", entries)
}

#[allow(dead_code)] // 外部调用
pub(crate) fn update_bool_section_document(
    content: &str,
    section_name: &str,
    key: &str,
    value: bool,
) -> String {
    let mut document = TomlDocument::new(content);
    let table_start = document.ensure_table(section_name);
    upsert_table_key(&mut document.lines, table_start, key, value.to_string());
    document.finish()
}

pub(crate) fn update_llm_document(content: &str, patch: &LlmConfigPatch) -> String {
    let mut document = TomlDocument::new(content);
    let table_start = document.ensure_table("llm");

    if let Some(enabled) = patch.enabled {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "enabled",
            enabled.to_string(),
        );
    }
    if let Some(provider) = patch.provider.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "provider",
            quote_toml_string(provider),
        );
    }
    if let Some(base_url) = patch.base_url.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "base_url",
            quote_toml_string(base_url),
        );
    }
    if let Some(provider_urls) = patch.provider_urls.as_ref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "provider_urls",
            render_toml_string_list(provider_urls),
        );
    }
    if let Some(model) = patch.model.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "model",
            quote_toml_string(model),
        );
    }
    if let Some(api_keys) = patch.api_keys.as_ref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "api_keys",
            render_toml_string_list(api_keys),
        );
    }
    if let Some(timeout_seconds) = patch.timeout_seconds {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "timeout_seconds",
            timeout_seconds.to_string(),
        );
    }
    if let Some(headers) = patch.headers.as_ref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "headers",
            render_toml_string_map(headers),
        );
    }
    if let Some(active_provider_id) = patch.active_provider_id.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "active_provider_id",
            quote_toml_string(active_provider_id),
        );
    }
    if let Some(providers) = patch.providers.as_ref() {
        replace_toml_llm_provider_blocks(&mut document.lines, table_start, providers);
    }

    document.finish()
}

#[allow(dead_code)]
pub(crate) fn update_flow_local_agent_document(
    content: &str,
    patch: &FlowLocalAgentConfigPatch,
) -> String {
    let mut document = TomlDocument::new(content);
    let table_start = document.ensure_table("flow_local_agent");

    if let Some(enabled) = patch.enabled {
        upsert_table_key(&mut document.lines, table_start, "enabled", enabled.to_string());
    }
    if let Some(base_url) = patch.base_url.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "base_url",
            quote_toml_string(base_url),
        );
    }
    if let Some(token) = patch.token.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "token",
            quote_toml_string(token),
        );
    }
    if let Some(device_id) = patch.device_id.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "device_id",
            quote_toml_string(device_id),
        );
    }
    if let Some(device_name) = patch.device_name.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "device_name",
            quote_toml_string(device_name),
        );
    }
    if let Some(auto_connect) = patch.auto_connect {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "auto_connect",
            auto_connect.to_string(),
        );
    }
    if let Some(allowed_tools) = patch.allowed_tools.as_ref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "allowed_tools",
            render_toml_string_list(allowed_tools),
        );
    }
    if let Some(workspace_root) = patch.workspace_root.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "workspace_root",
            quote_toml_string(workspace_root),
        );
    }
    if let Some(command_timeout_seconds) = patch.command_timeout_seconds {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "command_timeout_seconds",
            command_timeout_seconds.to_string(),
        );
    }
    if let Some(approval_policy) = patch.approval_policy.as_deref() {
        upsert_table_key(
            &mut document.lines,
            table_start,
            "approval_policy",
            quote_toml_string(approval_policy),
        );
    }

    document.finish()
}

struct TomlDocument<'a> {
    newline: &'a str,
    trailing_newline: bool,
    lines: Vec<String>,
}

impl<'a> TomlDocument<'a> {
    fn new(content: &'a str) -> Self {
        Self {
            newline: detect_newline(content),
            trailing_newline: content.ends_with('\n'),
            lines: content.lines().map(|line| line.to_string()).collect(),
        }
    }

    fn finish(self) -> String {
        join_lines(&self.lines, self.newline, self.trailing_newline)
    }

    fn ensure_table(&mut self, table_name: &str) -> usize {
        self.ensure_named_table(&[table_name], table_name)
    }

    fn ensure_named_table(&mut self, names: &[&str], render_name: &str) -> usize {
        if let Some(index) = self.find_table(names) {
            return index;
        }

        append_blank_line_if_needed(&mut self.lines);
        self.lines.push(format!("[{render_name}]"));
        self.lines.len() - 1
    }

    fn find_table(&self, names: &[&str]) -> Option<usize> {
        self.lines.iter().position(|line| {
            let trimmed = line.trim();
            names.iter().any(|name| trimmed == format!("[{name}]"))
        })
    }
}

fn update_disabled_list_section(content: &str, section_name: &str, entries: &[String]) -> String {
    let mut document = TomlDocument::new(content);
    let table_start = document.ensure_table(section_name);
    upsert_table_key(
        &mut document.lines,
        table_start,
        "disabled",
        render_toml_string_array(entries),
    );
    document.finish()
}

fn upsert_table_key(lines: &mut Vec<String>, table_start: usize, key: &str, value: String) {
    let table_end = find_toml_table_end(lines, table_start);
    let existing = (table_start + 1..table_end).find(|&idx| {
        let trimmed = lines[idx].trim_start();
        trimmed.starts_with(key) && trimmed.contains('=')
    });
    let rendered = format!("{key} = {value}");
    if let Some(index) = existing {
        lines[index] = rendered;
    } else {
        lines.splice(table_end..table_end, vec![rendered]);
    }
}

fn replace_toml_llm_provider_blocks(
    lines: &mut Vec<String>,
    table_start: usize,
    providers: &[LlmManagedProviderConfig],
) {
    let existing_start = lines
        .iter()
        .enumerate()
        .skip(table_start + 1)
        .find_map(|(idx, line)| (line.trim() == "[[llm.providers]]").then_some(idx));

    if let Some(existing_start) = existing_start {
        let mut existing_end = existing_start;
        while existing_end < lines.len() {
            let trimmed = lines[existing_end].trim();
            if existing_end > existing_start
                && trimmed.starts_with('[')
                && trimmed != "[[llm.providers]]"
            {
                break;
            }
            existing_end += 1;
        }
        lines.splice(existing_start..existing_end, Vec::<String>::new());
    }

    if providers.is_empty() {
        return;
    }

    let insert_at = find_toml_table_end(lines, table_start);
    let mut rendered = Vec::new();
    if insert_at > 0 && !lines[insert_at - 1].trim().is_empty() {
        rendered.push(String::new());
    }
    for (index, provider) in providers.iter().enumerate() {
        if index > 0 {
            rendered.push(String::new());
        }
        rendered.extend(render_toml_managed_provider_block(provider));
    }
    lines.splice(insert_at..insert_at, rendered);
}

fn find_toml_table_end(lines: &[String], table_start: usize) -> usize {
    for (idx, line) in lines.iter().enumerate().skip(table_start + 1) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return idx;
        }
    }
    lines.len()
}

fn render_toml_string_list(entries: &[String]) -> String {
    render_toml_string_array(entries)
}

fn render_toml_string_map(entries: &HashMap<String, String>) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }

    let mut keys = entries.keys().collect::<Vec<_>>();
    keys.sort();
    let rendered = keys
        .into_iter()
        .map(|key| {
            let value = entries
                .get(key.as_str())
                .map(String::as_str)
                .unwrap_or_default();
            format!("{} = {}", quote_toml_string(key), quote_toml_string(value))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {rendered} }}")
}

fn render_toml_managed_models(models: &[LlmManagedModelConfig]) -> String {
    if models.is_empty() {
        return "[]".to_string();
    }

    let rendered = models
        .iter()
        .map(|model| {
            let id = model.id.as_deref().unwrap_or("");
            let enabled = model.enabled.unwrap_or(false);
            format!("{{ id = {}, enabled = {enabled} }}", quote_toml_string(id))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

fn render_toml_managed_provider_block(provider: &LlmManagedProviderConfig) -> Vec<String> {
    let mut lines = vec!["[[llm.providers]]".to_string()];

    if let Some(id) = provider.id.as_deref() {
        lines.push(format!("id = {}", quote_toml_string(id)));
    }
    if let Some(label) = provider.label.as_deref() {
        lines.push(format!("label = {}", quote_toml_string(label)));
    }
    if let Some(provider_id) = provider.provider.as_deref() {
        lines.push(format!("provider = {}", quote_toml_string(provider_id)));
    }
    if let Some(base_url) = provider.base_url.as_deref() {
        lines.push(format!("base_url = {}", quote_toml_string(base_url)));
    }
    if let Some(api_key) = provider.api_key.as_deref() {
        lines.push(format!("api_key = {}", quote_toml_string(api_key)));
    }
    if let Some(timeout_seconds) = provider.timeout_seconds {
        lines.push(format!("timeout_seconds = {timeout_seconds}"));
    }
    if let Some(headers) = provider.headers.as_ref() {
        lines.push(format!("headers = {}", render_toml_string_map(headers)));
    }
    if let Some(models) = provider.models.as_ref() {
        lines.push(format!("models = {}", render_toml_managed_models(models)));
    }

    lines
}

fn render_toml_string_array(entries: &[String]) -> String {
    if entries.is_empty() {
        return "[]".to_string();
    }
    let rendered = entries
        .iter()
        .map(|entry| quote_toml_string(entry))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

fn quote_toml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
