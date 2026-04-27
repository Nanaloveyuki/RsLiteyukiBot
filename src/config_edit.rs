use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use liteyukibot_core::{LogLevel, emit_console_log};

use crate::app_config::{LlmManagedModelConfig, LlmManagedProviderConfig};

static ATOMIC_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

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

#[allow(dead_code)]
pub fn persist_desktop_close_to_tray(path: &Path, close_to_tray: bool) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|raw| raw.to_str())
        .map(|raw| raw.to_ascii_lowercase());

    let updated = match ext.as_deref() {
        Some("yaml") | Some("yml") => {
            update_yaml_bool_section_document(&content, "desktop", "close_to_tray", close_to_tray)
        }
        Some("toml") => {
            update_toml_bool_section_document(&content, "desktop", "close_to_tray", close_to_tray)
        }
        _ => {
            let err = format!(
                "unsupported config extension for {} (expected .yaml/.yml/.toml)",
                path.display()
            );
            log_config_persist_failure(
                "desktop close behavior",
                path,
                format!("close_to_tray={close_to_tray}").as_str(),
                err.as_str(),
            );
            return Err(err);
        }
    };

    persist_rendered_config(
        "desktop close behavior",
        path,
        format!("close_to_tray={close_to_tray}").as_str(),
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
    pub timeout_seconds: Option<u64>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub active_provider_id: Option<String>,
    pub providers: Option<Vec<LlmManagedProviderConfig>>,
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

pub fn write_text_file_atomically(path: &Path, content: &str) -> Result<(), String> {
    let _lock = ATOMIC_WRITE_LOCK
        .lock()
        .map_err(|_| format!("atomic write lock poisoned for {}", path.display()))?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create config directory {}: {err}", parent.display()))?;
    }

    let temp_path = atomic_write_temp_path(path);
    let backup_path = atomic_write_backup_path(path);
    let mut file = std::fs::File::create(&temp_path)
        .map_err(|err| format!("failed to create temp file {}: {err}", temp_path.display()))?;
    file.write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|err| {
            let _ = std::fs::remove_file(&temp_path);
            format!("failed to flush temp file {}: {err}", temp_path.display())
        })?;
    drop(file);

    if backup_path.exists() {
        let _ = std::fs::remove_file(&backup_path);
    }
    if path.exists() {
        std::fs::rename(path, &backup_path).map_err(|err| {
            let _ = std::fs::remove_file(&temp_path);
            format!(
                "failed to stage existing config {} for replacement: {err}",
                path.display()
            )
        })?;
    }
    if let Err(err) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        if backup_path.exists() {
            let _ = std::fs::rename(&backup_path, path);
        }
        return Err(format!(
            "failed to replace config {} with staged file: {err}",
            path.display()
        ));
    }
    if backup_path.exists() {
        let _ = std::fs::remove_file(&backup_path);
    }
    Ok(())
}

fn persist_rendered_config(
    config_kind: &str,
    path: &Path,
    detail: &str,
    updated: String,
) -> Result<(), String> {
    match write_text_file_atomically(path, &updated) {
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

fn atomic_write_temp_path(path: &Path) -> PathBuf {
    atomic_write_sidecar_path(path, "tmp")
}

fn atomic_write_backup_path(path: &Path) -> PathBuf {
    atomic_write_sidecar_path(path, "bak")
}

fn atomic_write_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config");
    path.with_file_name(format!("{file_name}.{suffix}"))
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
    if let Some(timeout_seconds) = patch.timeout_seconds {
        fields.push(format!("timeout_seconds={timeout_seconds}"));
    }
    if let Some(headers) = patch.headers.as_ref() {
        fields.push(format!("headers={}", headers.len()));
    }
    if let Some(active_provider_id) = patch.active_provider_id.as_deref() {
        fields.push(format!("active_provider_id={active_provider_id}"));
    }
    if let Some(providers) = patch.providers.as_ref() {
        fields.push(format!("providers={}", providers.len()));
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
        .map(|keys| normalize_entries_preserve_order(keys));
    let headers = patch.headers.as_ref().map(normalize_headers_map);
    let timeout_seconds = patch.timeout_seconds.filter(|value| *value > 0);
    let active_provider_id = patch
        .active_provider_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let providers = patch
        .providers
        .as_ref()
        .map(|providers| normalize_managed_providers(providers.as_slice()))
        .map(|providers: Vec<LlmManagedProviderConfig>| {
            providers
                .into_iter()
                .filter(|provider| provider.id.is_some() || provider.base_url.is_some())
                .collect::<Vec<_>>()
        });

    LlmConfigPatch {
        enabled: patch.enabled,
        provider,
        base_url,
        provider_urls,
        model,
        api_keys,
        timeout_seconds,
        headers,
        active_provider_id,
        providers,
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

fn normalize_headers_map(
    headers: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if key.is_empty() || value.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect()
}

fn normalize_managed_providers(
    providers: &[LlmManagedProviderConfig],
) -> Vec<LlmManagedProviderConfig> {
    providers
        .iter()
        .map(|provider| {
            let id = provider
                .id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let label = provider
                .label
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let provider_id = provider
                .provider
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_ascii_lowercase());
            let base_url = provider
                .base_url
                .as_deref()
                .and_then(normalize_provider_url);
            let api_key = provider
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let timeout_seconds = provider.timeout_seconds.filter(|value| *value > 0);
            let headers = provider.headers.as_ref().map(normalize_headers_map);
            let models = provider
                .models
                .as_ref()
                .map(|models| normalize_managed_models(models.as_slice()));

            LlmManagedProviderConfig {
                id,
                label,
                provider: provider_id,
                base_url,
                api_key,
                timeout_seconds,
                headers,
                models,
            }
        })
        .collect()
}

fn normalize_managed_models(models: &[LlmManagedModelConfig]) -> Vec<LlmManagedModelConfig> {
    models
        .iter()
        .filter_map(|model| {
            let id = model
                .id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            id.map(|id| LlmManagedModelConfig {
                id: Some(id),
                enabled: Some(model.enabled.unwrap_or(false)),
            })
        })
        .collect()
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

#[allow(dead_code)]
fn update_yaml_bool_section_document(
    content: &str,
    section_name: &str,
    key: &str,
    value: bool,
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
        let key_index = (section_start + 1..section_end).find(|&idx| {
            let trimmed = lines[idx].trim_start();
            trimmed.starts_with(key) && trimmed.contains(':')
        });
        let rendered = render_yaml_bool_key(section_indent + 2, key, value);

        if let Some(index) = key_index {
            lines[index] = render_yaml_bool_key(leading_spaces(lines[index].as_str()), key, value);
        } else {
            lines.splice(section_end..section_end, vec![rendered]);
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("{section_name}:"));
        lines.push(render_yaml_bool_key(2, key, value));
    }

    join_lines(&lines, newline, trailing_newline)
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

#[allow(dead_code)]
fn render_yaml_bool_key(indent: usize, key: &str, value: bool) -> String {
    format!("{}{}: {}", " ".repeat(indent), key, value)
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

#[allow(dead_code)]
fn update_toml_bool_section_document(
    content: &str,
    section_name: &str,
    key: &str,
    value: bool,
) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    let value_line = format!("{key} = {value}");

    let section_index = lines
        .iter()
        .position(|line| line.trim() == format!("[{section_name}]"));

    if let Some(section_start) = section_index {
        let table_end = find_toml_table_end(&lines, section_start);
        let existing = (section_start + 1..table_end).find(|&idx| {
            let trimmed = lines[idx].trim_start();
            trimmed.starts_with(key) && trimmed.contains('=')
        });
        if let Some(index) = existing {
            lines[index] = value_line;
        } else {
            lines.splice(table_end..table_end, vec![value_line]);
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{section_name}]"));
        lines.push(value_line);
    }

    join_lines(&lines, newline, trailing_newline)
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
    if let Some(timeout_seconds) = patch.timeout_seconds {
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "timeout_seconds",
            &timeout_seconds.to_string(),
        );
    }
    if let Some(headers) = patch.headers.as_ref() {
        upsert_yaml_block(
            &mut lines,
            section_start,
            section_indent,
            "headers",
            render_yaml_string_map(section_indent + 2, "headers", headers),
        );
    }
    if let Some(active_provider_id) = patch.active_provider_id.as_deref() {
        let escaped = active_provider_id.replace('\'', "''");
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "active_provider_id",
            &format!("'{escaped}'"),
        );
    }
    if let Some(providers) = patch.providers.as_ref() {
        upsert_yaml_block(
            &mut lines,
            section_start,
            section_indent,
            "providers",
            render_yaml_managed_providers(section_indent + 2, "providers", providers),
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

fn render_yaml_string_map(
    indent: usize,
    key: &str,
    entries: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if entries.is_empty() {
        return vec![format!("{prefix}{key}: {{}}")];
    }

    let mut rendered = vec![format!("{prefix}{key}:")];
    let mut keys = entries.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    for key_name in keys {
        let value = entries
            .get(key_name.as_str())
            .cloned()
            .unwrap_or_default()
            .replace('\'', "''");
        let escaped_key = key_name.replace('\'', "''");
        rendered.push(format!("{prefix}  '{escaped_key}': '{value}'"));
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
            provider.id.as_deref().unwrap_or("").replace('\'', "''")
        ));

        if let Some(label) = provider.label.as_deref() {
            rendered.push(format!(
                "{nested_prefix}label: '{}'",
                label.replace('\'', "''")
            ));
        }
        if let Some(provider_id) = provider.provider.as_deref() {
            rendered.push(format!(
                "{nested_prefix}provider: '{}'",
                provider_id.replace('\'', "''")
            ));
        }
        if let Some(base_url) = provider.base_url.as_deref() {
            rendered.push(format!(
                "{nested_prefix}base_url: '{}'",
                base_url.replace('\'', "''")
            ));
        }
        if let Some(api_key) = provider.api_key.as_deref() {
            rendered.push(format!(
                "{nested_prefix}api_key: '{}'",
                api_key.replace('\'', "''")
            ));
        }
        if let Some(timeout_seconds) = provider.timeout_seconds {
            rendered.push(format!("{nested_prefix}timeout_seconds: {timeout_seconds}"));
        }
        if let Some(headers) = provider.headers.as_ref() {
            let mut keys = headers.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            if keys.is_empty() {
                rendered.push(format!("{nested_prefix}headers: {{}}"));
            } else {
                rendered.push(format!("{nested_prefix}headers:"));
                for key_name in keys {
                    let value = headers
                        .get(key_name.as_str())
                        .cloned()
                        .unwrap_or_default()
                        .replace('\'', "''");
                    rendered.push(format!(
                        "{nested_prefix}  '{}': '{}'",
                        key_name.replace('\'', "''"),
                        value
                    ));
                }
            }
        }
        if let Some(models) = provider.models.as_ref() {
            if models.is_empty() {
                rendered.push(format!("{nested_prefix}models: []"));
            } else {
                rendered.push(format!("{nested_prefix}models:"));
                for model in models {
                    rendered.push(format!(
                        "{nested_prefix}  - id: '{}'",
                        model.id.as_deref().unwrap_or("").replace('\'', "''")
                    ));
                    rendered.push(format!(
                        "{nested_prefix}    enabled: {}",
                        model.enabled.unwrap_or(false)
                    ));
                }
            }
        }
    }

    rendered
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
    if let Some(timeout_seconds) = patch.timeout_seconds {
        upsert_toml_llm_key(
            &mut lines,
            table_start,
            "timeout_seconds",
            &timeout_seconds.to_string(),
        );
    }
    if let Some(headers) = patch.headers.as_ref() {
        let value = render_toml_string_map(headers);
        upsert_toml_llm_key(&mut lines, table_start, "headers", &value);
    }
    if let Some(active_provider_id) = patch.active_provider_id.as_deref() {
        let escaped = active_provider_id
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        upsert_toml_llm_key(
            &mut lines,
            table_start,
            "active_provider_id",
            &format!("\"{escaped}\""),
        );
    }
    if let Some(providers) = patch.providers.as_ref() {
        replace_toml_llm_provider_blocks(&mut lines, table_start, providers);
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

fn render_toml_string_map(entries: &std::collections::HashMap<String, String>) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }

    let mut keys = entries.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    let rendered = keys
        .into_iter()
        .map(|key| {
            let escaped_key = key.replace('\\', "\\\\").replace('"', "\\\"");
            let escaped_value = entries
                .get(key.as_str())
                .cloned()
                .unwrap_or_default()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            format!("\"{escaped_key}\" = \"{escaped_value}\"")
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
            let id = model
                .id
                .as_deref()
                .unwrap_or("")
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            let enabled = model.enabled.unwrap_or(false);
            format!("{{ id = \"{id}\", enabled = {enabled} }}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

fn render_toml_managed_provider_block(provider: &LlmManagedProviderConfig) -> Vec<String> {
    let mut lines = vec!["[[llm.providers]]".to_string()];

    if let Some(id) = provider.id.as_deref() {
        let escaped = id.replace('\\', "\\\\").replace('"', "\\\"");
        lines.push(format!("id = \"{escaped}\""));
    }
    if let Some(label) = provider.label.as_deref() {
        let escaped = label.replace('\\', "\\\\").replace('"', "\\\"");
        lines.push(format!("label = \"{escaped}\""));
    }
    if let Some(provider_id) = provider.provider.as_deref() {
        let escaped = provider_id.replace('\\', "\\\\").replace('"', "\\\"");
        lines.push(format!("provider = \"{escaped}\""));
    }
    if let Some(base_url) = provider.base_url.as_deref() {
        let escaped = base_url.replace('\\', "\\\\").replace('"', "\\\"");
        lines.push(format!("base_url = \"{escaped}\""));
    }
    if let Some(api_key) = provider.api_key.as_deref() {
        let escaped = api_key.replace('\\', "\\\\").replace('"', "\\\"");
        lines.push(format!("api_key = \"{escaped}\""));
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
