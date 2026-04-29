use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use liteyukibot_core::{LogLevel, emit_console_log};

static ATOMIC_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Clone, Copy)]
pub(crate) enum ConfigFormat {
    Yaml,
    Toml,
}

pub(crate) fn persist_config_with_format<Y, T>(
    config_kind: &str,
    path: &Path,
    detail: &str,
    update_yaml: Y,
    update_toml: T,
) -> Result<(), String>
where
    Y: FnOnce(&str) -> String,
    T: FnOnce(&str) -> String,
{
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let Some(format) = detect_config_format(path) else {
        let err = unsupported_extension_error(path);
        log_config_persist_failure(config_kind, path, detail, err.as_str());
        return Err(err);
    };

    let updated = match format {
        ConfigFormat::Yaml => update_yaml(&content),
        ConfigFormat::Toml => update_toml(&content),
    };
    persist_rendered_config(config_kind, path, detail, updated)
}

pub fn write_text_file_atomically(path: &Path, content: &str) -> Result<(), String> {
    let _lock = ATOMIC_WRITE_LOCK
        .lock()
        .map_err(|_| format!("atomic write lock poisoned for {}", path.display()))?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create config directory {}: {err}",
                parent.display()
            )
        })?;
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

pub(crate) fn normalize_entries(entries: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = entries
        .iter()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(crate) fn escape_yaml_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

pub(crate) fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

pub(crate) fn detect_newline(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

pub(crate) fn join_lines(lines: &[String], newline: &str, trailing_newline: bool) -> String {
    let mut rendered = lines.join(newline);
    if trailing_newline || !rendered.ends_with(newline) {
        rendered.push_str(newline);
    }
    rendered
}

pub(crate) fn append_blank_line_if_needed(lines: &mut Vec<String>) {
    if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.push(String::new());
    }
}

fn detect_config_format(path: &Path) -> Option<ConfigFormat> {
    match path
        .extension()
        .and_then(|raw| raw.to_str())
        .map(|raw| raw.to_ascii_lowercase())
        .as_deref()
    {
        Some("yaml") | Some("yml") => Some(ConfigFormat::Yaml),
        Some("toml") => Some(ConfigFormat::Toml),
        _ => None,
    }
}

fn unsupported_extension_error(path: &Path) -> String {
    format!(
        "unsupported config extension for {} (expected .yaml/.yml/.toml)",
        path.display()
    )
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

pub(crate) fn log_config_persist_failure(config_kind: &str, path: &Path, detail: &str, err: &str) {
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
