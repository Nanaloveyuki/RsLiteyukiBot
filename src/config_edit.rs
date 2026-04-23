use std::path::Path;

use liteyukibot_core::{LogLevel, emit_console_log};

pub fn persist_onebot_v11_whitelist(path: &Path, entries: &[String]) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|raw| raw.to_str())
        .map(|raw| raw.to_ascii_lowercase());

    let normalized = normalize_entries(entries);
    let updated = match ext.as_deref() {
        Some("yaml") | Some("yml") => update_yaml_document(&content, &normalized),
        Some("toml") => update_toml_document(&content, &normalized),
        _ => {
            let err = format!(
                "unsupported config extension for {} (expected .yaml/.yml/.toml)",
                path.display()
            );
            log_config_persist_failure(
                "onebot-v11 whitelist",
                path,
                format!("entries={}", normalized.len()).as_str(),
                err.as_str(),
            );
            return Err(err);
        }
    };

    persist_rendered_config(
        "onebot-v11 whitelist",
        path,
        format!("entries={}", normalized.len()).as_str(),
        updated,
    )?;
    Ok(())
}

pub fn persist_disabled_commands(path: &Path, entries: &[String]) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|raw| raw.to_str())
        .map(|raw| raw.to_ascii_lowercase());

    let normalized = normalize_entries(entries);
    let updated = match ext.as_deref() {
        Some("yaml") | Some("yml") => update_yaml_commands_document(&content, &normalized),
        Some("toml") => update_toml_commands_document(&content, &normalized),
        _ => {
            let err = format!(
                "unsupported config extension for {} (expected .yaml/.yml/.toml)",
                path.display()
            );
            log_config_persist_failure(
                "disabled commands",
                path,
                format!("entries={}", normalized.len()).as_str(),
                err.as_str(),
            );
            return Err(err);
        }
    };

    persist_rendered_config(
        "disabled commands",
        path,
        format!("entries={}", normalized.len()).as_str(),
        updated,
    )?;
    Ok(())
}

pub fn persist_disabled_plugins(path: &Path, entries: &[String]) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|raw| raw.to_str())
        .map(|raw| raw.to_ascii_lowercase());

    let normalized = normalize_entries(entries);
    let updated = match ext.as_deref() {
        Some("yaml") | Some("yml") => update_yaml_plugins_document(&content, &normalized),
        Some("toml") => update_toml_plugins_document(&content, &normalized),
        _ => {
            let err = format!(
                "unsupported config extension for {} (expected .yaml/.yml/.toml)",
                path.display()
            );
            log_config_persist_failure(
                "disabled plugins",
                path,
                format!("entries={}", normalized.len()).as_str(),
                err.as_str(),
            );
            return Err(err);
        }
    };

    persist_rendered_config(
        "disabled plugins",
        path,
        format!("entries={}", normalized.len()).as_str(),
        updated,
    )?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct LlmConfigPatch {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub provider_urls: Option<Vec<String>>,
    pub model: Option<String>,
    pub api_keys: Option<Vec<String>>,
}

pub fn persist_llm_config(path: &Path, patch: &LlmConfigPatch) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|raw| raw.to_str())
        .map(|raw| raw.to_ascii_lowercase());
    let normalized_patch = normalize_llm_patch(patch);

    let updated = match ext.as_deref() {
        Some("yaml") | Some("yml") => update_yaml_llm_document(&content, &normalized_patch),
        Some("toml") => update_toml_llm_document(&content, &normalized_patch),
        _ => {
            let err = format!(
                "unsupported config extension for {} (expected .yaml/.yml/.toml)",
                path.display()
            );
            log_config_persist_failure(
                "llm config",
                path,
                describe_llm_patch(&normalized_patch).as_str(),
                err.as_str(),
            );
            return Err(err);
        }
    };

    persist_rendered_config(
        "llm config",
        path,
        describe_llm_patch(&normalized_patch).as_str(),
        updated,
    )?;
    Ok(())
}

fn persist_rendered_config(
    config_kind: &str,
    path: &Path,
    detail: &str,
    updated: String,
) -> Result<(), String> {
    match std::fs::write(path, updated) {
        Ok(()) => {
            emit_console_log(
                LogLevel::Info,
                "config.edit",
                format!("persisted {config_kind} to {} ({detail})", path.display()),
            );
            Ok(())
        }
        Err(err) => {
            let err = format!("failed to write config {}: {err}", path.display());
            log_config_persist_failure(config_kind, path, detail, err.as_str());
            Err(err)
        }
    }
}

fn log_config_persist_failure(config_kind: &str, path: &Path, detail: &str, err: &str) {
    emit_console_log(
        LogLevel::Error,
        "config.edit",
        format!(
            "failed to persist {config_kind} at {} ({detail}): {err}",
            path.display()
        ),
    );
}

fn describe_llm_patch(patch: &LlmConfigPatch) -> String {
    let mut fields = Vec::new();
    if let Some(enabled) = patch.enabled {
        fields.push(format!("enabled={enabled}"));
    }
    if let Some(provider) = patch.provider.as_deref() {
        fields.push(format!("provider={provider}"));
    }
    if let Some(base_url) = patch.base_url.as_deref() {
        fields.push(format!("base_url={base_url}"));
    }
    if let Some(provider_urls) = patch.provider_urls.as_ref() {
        fields.push(format!("provider_urls={}", provider_urls.len()));
    }
    if let Some(model) = patch.model.as_deref() {
        fields.push(format!("model={model}"));
    }
    if let Some(api_keys) = patch.api_keys.as_ref() {
        fields.push(format!("api_keys=<updated:{}>", api_keys.len()));
    }

    if fields.is_empty() {
        "fields=none".to_string()
    } else {
        fields.join(", ")
    }
}

fn normalize_entries(entries: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = entries
        .iter()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_llm_patch(patch: &LlmConfigPatch) -> LlmConfigPatch {
    let provider = patch
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let base_url = patch.base_url.as_deref().and_then(normalize_provider_url);
    let provider_urls = patch
        .provider_urls
        .as_ref()
        .map(|urls| normalize_provider_url_entries(urls));
    let model = patch
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let api_keys = patch
        .api_keys
        .as_ref()
        .map(|keys| normalize_entries_preserve_order(keys))
        .filter(|keys| !keys.is_empty());

    LlmConfigPatch {
        enabled: patch.enabled,
        provider,
        base_url,
        provider_urls,
        model,
        api_keys,
    }
}

fn normalize_provider_url(raw: &str) -> Option<String> {
    let value = raw.trim().trim_end_matches('/').to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn normalize_provider_url_entries(entries: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();
    for raw in entries {
        let Some(value) = normalize_provider_url(raw.as_str()) else {
            continue;
        };
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

fn normalize_entries_preserve_order(entries: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();
    for raw in entries {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let value = raw.to_string();
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

fn update_yaml_document(content: &str, entries: &[String]) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();

    let section_index = lines.iter().position(|line| {
        matches!(
            line.trim(),
            "onebot-v11:" | "'onebot-v11':" | "\"onebot-v11\":"
        )
    });

    if let Some(section_start) = section_index {
        let section_indent = leading_spaces(lines[section_start].as_str());
        let section_end = find_yaml_section_end(&lines, section_start, section_indent);
        let whitelist_index = (section_start + 1..section_end).find(|&idx| {
            let trimmed = lines[idx].trim_start();
            trimmed.starts_with("whitelist:")
        });

        if let Some(index) = whitelist_index {
            let whitelist_indent = leading_spaces(lines[index].as_str());
            let mut block_end = index + 1;
            while block_end < section_end {
                let line = lines[block_end].as_str();
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                let indent = leading_spaces(line);
                if indent <= whitelist_indent {
                    break;
                }
                let trimmed_start = line.trim_start();
                if trimmed_start.starts_with('-') || trimmed_start.starts_with('#') {
                    block_end += 1;
                    continue;
                }
                break;
            }
            lines.splice(
                index..block_end,
                render_yaml_whitelist(whitelist_indent, entries),
            );
        } else {
            lines.splice(
                section_end..section_end,
                render_yaml_whitelist(section_indent + 2, entries),
            );
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("onebot-v11:".to_string());
        lines.extend(render_yaml_whitelist(2, entries));
    }

    join_lines(&lines, newline, trailing_newline)
}

fn update_yaml_commands_document(content: &str, entries: &[String]) -> String {
    update_yaml_disabled_list_section(content, "commands", entries)
}

fn update_yaml_plugins_document(content: &str, entries: &[String]) -> String {
    update_yaml_disabled_list_section(content, "plugins", entries)
}

fn update_yaml_disabled_list_section(
    content: &str,
    section_name: &str,
    entries: &[String],
) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();

    let section_index = lines.iter().position(|line| {
        let trimmed = line.trim();
        trimmed == format!("{section_name}:")
            || trimmed == format!("'{section_name}':")
            || trimmed == format!("\"{section_name}\":")
    });

    if let Some(section_start) = section_index {
        let section_indent = leading_spaces(lines[section_start].as_str());
        let section_end = find_yaml_section_end(&lines, section_start, section_indent);
        let disabled_index = (section_start + 1..section_end).find(|&idx| {
            let trimmed = lines[idx].trim_start();
            trimmed.starts_with("disabled:")
        });

        if let Some(index) = disabled_index {
            let disabled_indent = leading_spaces(lines[index].as_str());
            let mut block_end = index + 1;
            while block_end < section_end {
                let line = lines[block_end].as_str();
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                let indent = leading_spaces(line);
                if indent <= disabled_indent {
                    break;
                }
                let trimmed_start = line.trim_start();
                if trimmed_start.starts_with('-') || trimmed_start.starts_with('#') {
                    block_end += 1;
                    continue;
                }
                break;
            }
            lines.splice(
                index..block_end,
                render_yaml_disabled_commands(disabled_indent, entries),
            );
        } else {
            lines.splice(
                section_end..section_end,
                render_yaml_disabled_commands(section_indent + 2, entries),
            );
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("{section_name}:"));
        lines.extend(render_yaml_disabled_commands(2, entries));
    }

    join_lines(&lines, newline, trailing_newline)
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

fn render_yaml_whitelist(indent: usize, entries: &[String]) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if entries.is_empty() {
        return vec![format!("{prefix}whitelist: []")];
    }
    let mut lines = vec![format!("{prefix}whitelist:")];
    for entry in entries {
        let escaped = entry.replace('\'', "''");
        lines.push(format!("{prefix}  - '{escaped}'"));
    }
    lines
}

fn render_yaml_disabled_commands(indent: usize, entries: &[String]) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if entries.is_empty() {
        return vec![format!("{prefix}disabled: []")];
    }
    let mut lines = vec![format!("{prefix}disabled:")];
    let item_prefix = " ".repeat(indent + 2);
    for entry in entries {
        lines.push(format!(
            "{item_prefix}- '{}'",
            escape_yaml_single_quoted(entry)
        ));
    }
    lines
}

fn update_toml_document(content: &str, entries: &[String]) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    let table_index = lines
        .iter()
        .position(|line| matches!(line.trim(), "[onebot-v11]" | "[onebot_v11]"));
    let whitelist_line = render_toml_whitelist(entries);

    if let Some(table_start) = table_index {
        let table_end = find_toml_table_end(&lines, table_start);
        let existing = (table_start + 1..table_end).find(|&idx| {
            let trimmed = lines[idx].trim_start();
            trimmed.starts_with("whitelist") && trimmed.contains('=')
        });
        if let Some(index) = existing {
            lines[index] = whitelist_line;
        } else {
            lines.splice(table_end..table_end, vec![whitelist_line]);
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("[onebot-v11]".to_string());
        lines.push(whitelist_line);
    }

    join_lines(&lines, newline, trailing_newline)
}

fn update_toml_commands_document(content: &str, entries: &[String]) -> String {
    update_toml_disabled_list_section(content, "commands", entries)
}

fn update_toml_plugins_document(content: &str, entries: &[String]) -> String {
    update_toml_disabled_list_section(content, "plugins", entries)
}

fn update_toml_disabled_list_section(
    content: &str,
    section_name: &str,
    entries: &[String],
) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    let disabled_line = render_toml_disabled_commands(entries);

    let section_index = lines
        .iter()
        .position(|line| line.trim() == format!("[{section_name}]"));

    if let Some(section_start) = section_index {
        let table_end = find_toml_table_end(&lines, section_start);
        let disabled_index = (section_start + 1..table_end).find(|&idx| {
            let trimmed = lines[idx].trim_start();
            trimmed.starts_with("disabled") && trimmed.contains('=')
        });
        if let Some(index) = disabled_index {
            lines[index] = disabled_line;
        } else {
            lines.splice(table_end..table_end, vec![disabled_line]);
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{section_name}]"));
        lines.push(disabled_line);
    }

    join_lines(&lines, newline, trailing_newline)
}

fn update_yaml_llm_document(content: &str, patch: &LlmConfigPatch) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();

    let section_index = lines
        .iter()
        .position(|line| matches!(line.trim(), "llm:" | "'llm':" | "\"llm\":"));
    let section_start = if let Some(index) = section_index {
        index
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("llm:".to_string());
        lines.len() - 1
    };
    let section_indent = leading_spaces(lines[section_start].as_str());

    if let Some(enabled) = patch.enabled {
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "enabled",
            &enabled.to_string(),
        );
    }
    if let Some(provider) = patch.provider.as_deref() {
        let escaped = provider.replace('\'', "''");
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "provider",
            &format!("'{escaped}'"),
        );
    }
    if let Some(base_url) = patch.base_url.as_deref() {
        let escaped = base_url.replace('\'', "''");
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "base_url",
            &format!("'{escaped}'"),
        );
    }
    if let Some(provider_urls) = patch.provider_urls.as_ref() {
        upsert_yaml_list(
            &mut lines,
            section_start,
            section_indent,
            "provider_urls",
            provider_urls,
        );
    }
    if let Some(model) = patch.model.as_deref() {
        let escaped = model.replace('\'', "''");
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "model",
            &format!("'{escaped}'"),
        );
    }
    if let Some(api_keys) = patch.api_keys.as_ref() {
        upsert_yaml_list(
            &mut lines,
            section_start,
            section_indent,
            "api_keys",
            api_keys,
        );
    }

    join_lines(&lines, newline, trailing_newline)
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

fn render_yaml_string_list(indent: usize, key: &str, entries: &[String]) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if entries.is_empty() {
        return vec![format!("{prefix}{key}: []")];
    }
    let mut lines = vec![format!("{prefix}{key}:")];
    for entry in entries {
        let escaped = entry.replace('\'', "''");
        lines.push(format!("{prefix}  - '{escaped}'"));
    }
    lines
}

fn update_toml_llm_document(content: &str, patch: &LlmConfigPatch) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();

    let table_index = lines.iter().position(|line| line.trim() == "[llm]");
    let table_start = if let Some(index) = table_index {
        index
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("[llm]".to_string());
        lines.len() - 1
    };

    if let Some(enabled) = patch.enabled {
        upsert_toml_llm_key(&mut lines, table_start, "enabled", &enabled.to_string());
    }
    if let Some(provider) = patch.provider.as_deref() {
        let escaped = provider.replace('\\', "\\\\").replace('"', "\\\"");
        upsert_toml_llm_key(
            &mut lines,
            table_start,
            "provider",
            &format!("\"{escaped}\""),
        );
    }
    if let Some(base_url) = patch.base_url.as_deref() {
        let escaped = base_url.replace('\\', "\\\\").replace('"', "\\\"");
        upsert_toml_llm_key(
            &mut lines,
            table_start,
            "base_url",
            &format!("\"{escaped}\""),
        );
    }
    if let Some(provider_urls) = patch.provider_urls.as_ref() {
        let value = render_toml_string_list(provider_urls);
        upsert_toml_llm_key(&mut lines, table_start, "provider_urls", &value);
    }
    if let Some(model) = patch.model.as_deref() {
        let escaped = model.replace('\\', "\\\\").replace('"', "\\\"");
        upsert_toml_llm_key(&mut lines, table_start, "model", &format!("\"{escaped}\""));
    }
    if let Some(api_keys) = patch.api_keys.as_ref() {
        let value = render_toml_string_list(api_keys);
        upsert_toml_llm_key(&mut lines, table_start, "api_keys", &value);
    }

    join_lines(&lines, newline, trailing_newline)
}

fn upsert_toml_llm_key(lines: &mut Vec<String>, table_start: usize, key: &str, value: &str) {
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

fn render_toml_string_list(entries: &[String]) -> String {
    if entries.is_empty() {
        return "[]".to_string();
    }
    let rendered = entries
        .iter()
        .map(|entry| {
            let escaped = entry.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
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

fn render_toml_whitelist(entries: &[String]) -> String {
    format!("whitelist = {}", render_toml_string_array(entries))
}

fn render_toml_disabled_commands(entries: &[String]) -> String {
    format!("disabled = {}", render_toml_string_array(entries))
}

fn render_toml_string_array(entries: &[String]) -> String {
    if entries.is_empty() {
        return "[]".to_string();
    }
    let rendered = entries
        .iter()
        .map(|entry| {
            let escaped = entry.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

fn escape_yaml_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

fn detect_newline(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn join_lines(lines: &[String], newline: &str, trailing_newline: bool) -> String {
    let mut rendered = lines.join(newline);
    if trailing_newline || !rendered.ends_with(newline) {
        rendered.push_str(newline);
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use liteyukibot_core::recent_buffered_logs;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

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
        let updated =
            update_yaml_document(source, &["private:1".to_string(), "group:2".to_string()]);
        assert!(
            updated.contains("onebot-v11:\n  whitelist:\n    - 'private:1'\n    - 'group:2'\n")
        );
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
            updated.contains(
                "commands:\n  disabled:\n    - 'adapter:onebot11 /su'\n    - 'tui /help'\n"
            )
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
            updated
                .contains("plugins:\n  disabled:\n    - 'builtin-liteecho'\n    - 'demo-plugin'\n")
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

        persist_disabled_commands(&path, &["tui /help".to_string()])
            .expect("persist should succeed");

        let path_display = path.display().to_string();
        assert!(recent_buffered_logs(50).iter().any(|entry| {
            entry.module == "config.edit"
                && entry.message.contains("persisted disabled commands")
                && entry.message.contains(path_display.as_str())
                && entry.message.contains("entries=1")
        }));

        let _ = fs::remove_file(path);
    }
}
