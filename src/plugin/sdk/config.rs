use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde_json::{Map, Value};

use crate::plugin::{PluginDescriptor, PluginSdkError};
use crate::utils::config_path::{
    resolve_default_app_config_path, resolve_existing_app_config_path,
};

pub(super) static PLUGIN_CONFIG_RW_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone, Copy)]
pub(super) enum ConfigFormat {
    Yaml,
    Toml,
}

pub(super) fn read_config_value(
    config_path: Option<&Path>,
    key: &str,
) -> Result<Option<Value>, String> {
    let _guard = PLUGIN_CONFIG_RW_LOCK
        .lock()
        .map_err(|_| "plugin config lock poisoned".to_string())?;
    let (_, _, document) = read_plugin_config_document(config_path)?;
    let segments = parse_config_path_segments(key)?;
    Ok(get_value_from_path(&document, &segments).cloned())
}

pub(super) fn write_config_value(
    config_path: Option<&Path>,
    key: &str,
    value: Value,
) -> Result<(), String> {
    let _guard = PLUGIN_CONFIG_RW_LOCK
        .lock()
        .map_err(|_| "plugin config lock poisoned".to_string())?;
    let (path, format, mut document) = read_plugin_config_document(config_path)?;
    let segments = parse_config_path_segments(key)?;
    set_value_at_path(&mut document, &segments, value)?;
    write_plugin_config_document(path.as_path(), format, &document)
}

pub(super) fn delete_config_value(config_path: Option<&Path>, key: &str) -> Result<bool, String> {
    let _guard = PLUGIN_CONFIG_RW_LOCK
        .lock()
        .map_err(|_| "plugin config lock poisoned".to_string())?;
    let (path, format, mut document) = read_plugin_config_document(config_path)?;
    let segments = parse_config_path_segments(key)?;
    let changed = delete_value_at_path(&mut document, &segments);
    if changed {
        write_plugin_config_document(path.as_path(), format, &document)?;
    }
    Ok(changed)
}

pub(super) fn read_plugin_config_document(
    config_path: Option<&Path>,
) -> Result<(PathBuf, ConfigFormat, Value), String> {
    let path = resolve_plugin_config_path(config_path);
    let format = detect_config_format(path.as_path());
    if !path.exists() {
        return Ok((path, format, Value::Object(Map::new())));
    }
    let content = fs::read_to_string(path.as_path())
        .map_err(|err| format!("failed to read config {}: {}", path.display(), err))?;
    let value = match format {
        ConfigFormat::Yaml => {
            let yaml_value =
                serde_yaml::from_str::<serde_yaml::Value>(content.as_str()).map_err(|err| {
                    format!("failed to parse yaml config {}: {}", path.display(), err)
                })?;
            serde_json::to_value(yaml_value)
                .map_err(|err| format!("failed to convert yaml config to json: {}", err))?
        }
        ConfigFormat::Toml => {
            let toml_value = toml::from_str::<toml::Value>(content.as_str()).map_err(|err| {
                format!("failed to parse toml config {}: {}", path.display(), err)
            })?;
            serde_json::to_value(toml_value)
                .map_err(|err| format!("failed to convert toml config to json: {}", err))?
        }
    };
    Ok((path, format, value))
}

pub(super) fn write_plugin_config_document(
    path: &Path,
    format: ConfigFormat,
    value: &Value,
) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create config parent directory {}: {}",
                parent.display(),
                err
            )
        })?;
    }
    let content = match format {
        ConfigFormat::Yaml => serde_yaml::to_string(value)
            .map_err(|err| format!("failed to serialize yaml config: {}", err))?,
        ConfigFormat::Toml => toml::to_string_pretty(value)
            .map_err(|err| format!("failed to serialize toml config: {}", err))?,
    };
    fs::write(path, content)
        .map_err(|err| format!("failed to write config {}: {}", path.display(), err))
}

fn resolve_plugin_config_path(config_path: Option<&Path>) -> PathBuf {
    if let Some(path) = config_path {
        return path.to_path_buf();
    }
    if let Ok(path) = std::env::var("LY_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    if let Some(path) = resolve_existing_app_config_path() {
        return path;
    }
    resolve_default_app_config_path()
}

pub(super) fn resolve_declared_plugin_config_path(
    descriptor: &PluginDescriptor,
) -> Result<PathBuf, PluginSdkError> {
    descriptor
        .runtime
        .options
        .get("config_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            PluginSdkError::Runtime(format!(
                "plugin '{}' does not declare runtime.options.config_path",
                descriptor.metadata.id
            ))
        })
}

pub(super) fn detect_config_format(path: &Path) -> ConfigFormat {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("toml") => ConfigFormat::Toml,
        _ => ConfigFormat::Yaml,
    }
}

fn parse_config_path_segments(key: &str) -> Result<Vec<&str>, String> {
    let key = key.trim();
    if key.is_empty() {
        return Ok(Vec::new());
    }
    let segments: Vec<&str> = key
        .split('.')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return Err("config path should not be empty".to_string());
    }
    Ok(segments)
}

fn get_value_from_path<'a>(value: &'a Value, segments: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in segments {
        cursor = cursor.as_object()?.get(*segment)?;
    }
    Some(cursor)
}

fn set_value_at_path(value: &mut Value, segments: &[&str], patch: Value) -> Result<(), String> {
    if segments.is_empty() {
        *value = patch;
        return Ok(());
    }
    let mut cursor = value;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        if !cursor.is_object() {
            *cursor = Value::Object(Map::new());
        }
        let map = cursor
            .as_object_mut()
            .ok_or_else(|| "failed to convert config node to object".to_string())?;
        cursor = map
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !cursor.is_object() {
        *cursor = Value::Object(Map::new());
    }
    let map = cursor
        .as_object_mut()
        .ok_or_else(|| "failed to convert config node to object".to_string())?;
    map.insert(
        segments
            .last()
            .ok_or_else(|| "config path should not be empty".to_string())?
            .to_string(),
        patch,
    );
    Ok(())
}

fn delete_value_at_path(value: &mut Value, segments: &[&str]) -> bool {
    if segments.is_empty() {
        return false;
    }
    let mut cursor = value;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        let Some(map) = cursor.as_object_mut() else {
            return false;
        };
        let Some(next) = map.get_mut(*segment) else {
            return false;
        };
        cursor = next;
    }
    let Some(map) = cursor.as_object_mut() else {
        return false;
    };
    map.remove(
        *segments
            .last()
            .expect("segments should contain at least one item"),
    )
    .is_some()
}
