use std::collections::HashMap;

use crate::app_config::{LlmManagedModelConfig, LlmManagedProviderConfig};

use super::llm::LlmConfigPatch;
use super::shared::{
    append_blank_line_if_needed, detect_newline, escape_yaml_single_quoted, join_lines,
    leading_spaces,
};

pub(crate) fn update_whitelist_document(content: &str, entries: &[String]) -> String {
    let mut document = YamlDocument::new(content);
    let (section_start, section_indent) = document.ensure_section("onebot-v11");
    upsert_yaml_list_block(
        &mut document.lines,
        section_start,
        section_indent,
        "whitelist",
        render_yaml_keyed_list(section_indent + 2, "whitelist", entries),
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
    let mut document = YamlDocument::new(content);
    let (section_start, section_indent) = document.ensure_section(section_name);
    upsert_yaml_scalar(
        &mut document.lines,
        section_start,
        section_indent,
        key,
        &value.to_string(),
    );
    document.finish()
}

pub(crate) fn update_llm_document(content: &str, patch: &LlmConfigPatch) -> String {
    let mut document = YamlDocument::new(content);
    let (section_start, section_indent) = document.ensure_section("llm");

    if let Some(enabled) = patch.enabled {
        upsert_yaml_scalar(
            &mut document.lines,
            section_start,
            section_indent,
            "enabled",
            &enabled.to_string(),
        );
    }
    if let Some(provider) = patch.provider.as_deref() {
        let escaped = escape_yaml_single_quoted(provider);
        upsert_yaml_scalar(
            &mut document.lines,
            section_start,
            section_indent,
            "provider",
            &format!("'{escaped}'"),
        );
    }
    if let Some(base_url) = patch.base_url.as_deref() {
        let escaped = escape_yaml_single_quoted(base_url);
        upsert_yaml_scalar(
            &mut document.lines,
            section_start,
            section_indent,
            "base_url",
            &format!("'{escaped}'"),
        );
    }
    if let Some(provider_urls) = patch.provider_urls.as_ref() {
        upsert_yaml_list(
            &mut document.lines,
            section_start,
            section_indent,
            "provider_urls",
            provider_urls,
        );
    }
    if let Some(model) = patch.model.as_deref() {
        let escaped = escape_yaml_single_quoted(model);
        upsert_yaml_scalar(
            &mut document.lines,
            section_start,
            section_indent,
            "model",
            &format!("'{escaped}'"),
        );
    }
    if let Some(api_keys) = patch.api_keys.as_ref() {
        upsert_yaml_list(
            &mut document.lines,
            section_start,
            section_indent,
            "api_keys",
            api_keys,
        );
    }
    if let Some(timeout_seconds) = patch.timeout_seconds {
        upsert_yaml_scalar(
            &mut document.lines,
            section_start,
            section_indent,
            "timeout_seconds",
            &timeout_seconds.to_string(),
        );
    }
    if let Some(headers) = patch.headers.as_ref() {
        upsert_yaml_block(
            &mut document.lines,
            section_start,
            section_indent,
            "headers",
            render_yaml_string_map(section_indent + 2, "headers", headers),
        );
    }
    if let Some(active_provider_id) = patch.active_provider_id.as_deref() {
        let escaped = escape_yaml_single_quoted(active_provider_id);
        upsert_yaml_scalar(
            &mut document.lines,
            section_start,
            section_indent,
            "active_provider_id",
            &format!("'{escaped}'"),
        );
    }
    if let Some(providers) = patch.providers.as_ref() {
        upsert_yaml_block(
            &mut document.lines,
            section_start,
            section_indent,
            "providers",
            render_yaml_managed_providers(section_indent + 2, "providers", providers),
        );
    }

    document.finish()
}

struct YamlDocument<'a> {
    newline: &'a str,
    trailing_newline: bool,
    lines: Vec<String>,
}

impl<'a> YamlDocument<'a> {
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

    fn ensure_section(&mut self, section_name: &str) -> (usize, usize) {
        if let Some(index) = self.find_section(section_name) {
            let indent = leading_spaces(self.lines[index].as_str());
            return (index, indent);
        }

        append_blank_line_if_needed(&mut self.lines);
        self.lines.push(format!("{section_name}:"));
        (self.lines.len() - 1, 0)
    }

    fn find_section(&self, section_name: &str) -> Option<usize> {
        self.lines.iter().position(|line| {
            let trimmed = line.trim();
            trimmed == format!("{section_name}:")
                || trimmed == format!("'{section_name}':")
                || trimmed == format!("\"{section_name}\":")
        })
    }
}

fn update_disabled_list_section(content: &str, section_name: &str, entries: &[String]) -> String {
    let mut document = YamlDocument::new(content);
    let (section_start, section_indent) = document.ensure_section(section_name);
    upsert_yaml_list_block(
        &mut document.lines,
        section_start,
        section_indent,
        "disabled",
        render_yaml_keyed_list(section_indent + 2, "disabled", entries),
    );
    document.finish()
}

fn upsert_yaml_list_block(
    lines: &mut Vec<String>,
    section_start: usize,
    section_indent: usize,
    key: &str,
    rendered: Vec<String>,
) {
    let section_end = find_yaml_section_end(lines, section_start, section_indent);
    let key_index = (section_start + 1..section_end).find(|&idx| {
        let trimmed = lines[idx].trim_start();
        trimmed.starts_with(&format!("{key}:"))
    });

    if let Some(index) = key_index {
        let key_indent = leading_spaces(lines[index].as_str());
        let block_end = find_yaml_list_block_end(lines, index, section_end, key_indent);
        lines.splice(index..block_end, rendered);
    } else {
        lines.splice(section_end..section_end, rendered);
    }
}

fn find_yaml_list_block_end(
    lines: &[String],
    key_index: usize,
    section_end: usize,
    key_indent: usize,
) -> usize {
    let mut block_end = key_index + 1;
    while block_end < section_end {
        let line = lines[block_end].as_str();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        let indent = leading_spaces(line);
        if indent <= key_indent {
            break;
        }
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with('-') || trimmed_start.starts_with('#') {
            block_end += 1;
            continue;
        }
        break;
    }
    block_end
}

fn upsert_yaml_scalar(
    lines: &mut Vec<String>,
    section_start: usize,
    section_indent: usize,
    key: &str,
    value: &str,
) {
    let section_end = find_yaml_section_end(lines, section_start, section_indent);
    let key_indent = section_indent + 2;
    let key_index = (section_start + 1..section_end).find(|&idx| {
        let line = lines[idx].as_str();
        leading_spaces(line) == key_indent && line.trim_start().starts_with(&format!("{key}:"))
    });

    let rendered = format!("{}{}: {}", " ".repeat(key_indent), key, value);
    if let Some(index) = key_index {
        let block_end = find_yaml_key_block_end(lines, index, section_end);
        lines.splice(index..block_end, vec![rendered]);
    } else {
        lines.splice(section_end..section_end, vec![rendered]);
    }
}

fn upsert_yaml_list(
    lines: &mut Vec<String>,
    section_start: usize,
    section_indent: usize,
    key: &str,
    entries: &[String],
) {
    let section_end = find_yaml_section_end(lines, section_start, section_indent);
    let key_indent = section_indent + 2;
    let key_index = (section_start + 1..section_end).find(|&idx| {
        let line = lines[idx].as_str();
        leading_spaces(line) == key_indent && line.trim_start().starts_with(&format!("{key}:"))
    });

    let rendered = render_yaml_string_list(key_indent, key, entries);
    if let Some(index) = key_index {
        let block_end = find_yaml_key_block_end(lines, index, section_end);
        lines.splice(index..block_end, rendered);
    } else {
        lines.splice(section_end..section_end, rendered);
    }
}

fn upsert_yaml_block(
    lines: &mut Vec<String>,
    section_start: usize,
    section_indent: usize,
    key: &str,
    rendered: Vec<String>,
) {
    let section_end = find_yaml_section_end(lines, section_start, section_indent);
    let key_indent = section_indent + 2;
    let key_index = (section_start + 1..section_end).find(|&idx| {
        let line = lines[idx].as_str();
        leading_spaces(line) == key_indent && line.trim_start().starts_with(&format!("{key}:"))
    });

    if let Some(index) = key_index {
        let block_end = find_yaml_key_block_end(lines, index, section_end);
        lines.splice(index..block_end, rendered);
    } else {
        lines.splice(section_end..section_end, rendered);
    }
}

fn find_yaml_key_block_end(lines: &[String], key_index: usize, section_end: usize) -> usize {
    let key_indent = leading_spaces(lines[key_index].as_str());
    let mut block_end = key_index + 1;
    while block_end < section_end {
        let line = lines[block_end].as_str();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            block_end += 1;
            continue;
        }
        let indent = leading_spaces(line);
        if indent <= key_indent {
            break;
        }
        block_end += 1;
    }
    block_end
}

fn find_yaml_section_end(lines: &[String], section_start: usize, section_indent: usize) -> usize {
    for (idx, line) in lines.iter().enumerate().skip(section_start + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = leading_spaces(line.as_str());
        if indent <= section_indent {
            return idx;
        }
    }
    lines.len()
}

fn render_yaml_keyed_list(indent: usize, key: &str, entries: &[String]) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if entries.is_empty() {
        return vec![format!("{prefix}{key}: []")];
    }

    let mut lines = vec![format!("{prefix}{key}:")];
    let item_prefix = " ".repeat(indent + 2);
    for entry in entries {
        lines.push(format!(
            "{item_prefix}- '{}'",
            escape_yaml_single_quoted(entry)
        ));
    }
    lines
}

fn render_yaml_string_list(indent: usize, key: &str, entries: &[String]) -> Vec<String> {
    render_yaml_keyed_list(indent, key, entries)
}

fn render_yaml_string_map(
    indent: usize,
    key: &str,
    entries: &HashMap<String, String>,
) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if entries.is_empty() {
        return vec![format!("{prefix}{key}: {{}}")];
    }

    let mut rendered = vec![format!("{prefix}{key}:")];
    let mut keys = entries.keys().collect::<Vec<_>>();
    keys.sort();
    for key_name in keys {
        let value = entries
            .get(key_name.as_str())
            .map(String::as_str)
            .unwrap_or_default();
        rendered.push(format!(
            "{prefix}  '{}': '{}'",
            escape_yaml_single_quoted(key_name),
            escape_yaml_single_quoted(value)
        ));
    }
    rendered
}

fn render_yaml_managed_providers(
    indent: usize,
    key: &str,
    providers: &[LlmManagedProviderConfig],
) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if providers.is_empty() {
        return vec![format!("{prefix}{key}: []")];
    }

    let mut rendered = vec![format!("{prefix}{key}:")];
    for provider in providers {
        let item_prefix = format!("{prefix}  -");
        let nested_prefix = format!("{prefix}    ");
        rendered.push(format!(
            "{item_prefix} id: '{}'",
            escape_yaml_single_quoted(provider.id.as_deref().unwrap_or(""))
        ));

        if let Some(label) = provider.label.as_deref() {
            rendered.push(format!(
                "{nested_prefix}label: '{}'",
                escape_yaml_single_quoted(label)
            ));
        }
        if let Some(provider_id) = provider.provider.as_deref() {
            rendered.push(format!(
                "{nested_prefix}provider: '{}'",
                escape_yaml_single_quoted(provider_id)
            ));
        }
        if let Some(base_url) = provider.base_url.as_deref() {
            rendered.push(format!(
                "{nested_prefix}base_url: '{}'",
                escape_yaml_single_quoted(base_url)
            ));
        }
        if let Some(api_key) = provider.api_key.as_deref() {
            rendered.push(format!(
                "{nested_prefix}api_key: '{}'",
                escape_yaml_single_quoted(api_key)
            ));
        }
        if let Some(timeout_seconds) = provider.timeout_seconds {
            rendered.push(format!("{nested_prefix}timeout_seconds: {timeout_seconds}"));
        }
        if let Some(headers) = provider.headers.as_ref() {
            let mut keys = headers.keys().collect::<Vec<_>>();
            keys.sort();
            if keys.is_empty() {
                rendered.push(format!("{nested_prefix}headers: {{}}"));
            } else {
                rendered.push(format!("{nested_prefix}headers:"));
                for key_name in keys {
                    let value = headers
                        .get(key_name.as_str())
                        .map(String::as_str)
                        .unwrap_or_default();
                    rendered.push(format!(
                        "{nested_prefix}  '{}': '{}'",
                        escape_yaml_single_quoted(key_name),
                        escape_yaml_single_quoted(value)
                    ));
                }
            }
        }
        if let Some(models) = provider.models.as_ref() {
            render_yaml_provider_models(&mut rendered, nested_prefix.as_str(), models);
        }
    }

    rendered
}

fn render_yaml_provider_models(
    rendered: &mut Vec<String>,
    nested_prefix: &str,
    models: &[LlmManagedModelConfig],
) {
    if models.is_empty() {
        rendered.push(format!("{nested_prefix}models: []"));
        return;
    }

    rendered.push(format!("{nested_prefix}models:"));
    for model in models {
        rendered.push(format!(
            "{nested_prefix}  - id: '{}'",
            escape_yaml_single_quoted(model.id.as_deref().unwrap_or(""))
        ));
        rendered.push(format!(
            "{nested_prefix}    enabled: {}",
            model.enabled.unwrap_or(false)
        ));
    }
}
