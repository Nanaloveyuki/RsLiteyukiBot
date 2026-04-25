use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pyo3::prelude::*;
use pyo3::types::PyAny;
use serde::Serialize;
use serde_json::Value;

use crate::plugin::PluginCommandDescriptor;

use super::lifecycle::{
    PythonDeclaredCommandEntry, PythonRuntimeState, PythonTuiCommandEntry, ScopedCommandKey,
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginTuiCommand {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginScopedCommand {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub plugin_id: String,
    pub scopes: Vec<String>,
    pub executable_in_tui: bool,
}

pub(crate) fn disabled_declared_command_for_plugin(
    state: &PythonRuntimeState,
    plugin_id: &str,
    payload: &Value,
) -> Option<String> {
    let scope = adapter_scope_for_payload(payload)?;
    let message = extract_payload_message_text(payload)?;
    state
        .declared_commands
        .iter()
        .find(|entry| {
            entry.plugin_id == plugin_id
                && is_scope_command_disabled(state, scope, entry.command.as_str())
                && plugin_scope_matches(&entry.scopes, scope)
                && declared_command_matches_message(entry.command.as_str(), message.as_str())
        })
        .map(|entry| entry.command.clone())
}

fn adapter_scope_for_payload(payload: &Value) -> Option<&'static str> {
    let object = payload.as_object()?;
    let protocol = object
        .get("_adapter_protocol")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let is_onebot_v11 = protocol.eq_ignore_ascii_case("onebot.v11")
        || object.contains_key("post_type")
        || object.contains_key("meta_event_type");
    if !is_onebot_v11 || object.get("post_type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    Some("adapter:onebot11")
}

fn extract_payload_message_text(payload: &Value) -> Option<String> {
    let object = payload.as_object()?;
    for key in ["raw_message", "text"] {
        if let Some(text) = object.get(key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    if let Some(text) = object.get("message").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let segments = object.get("message")?.as_array()?;
    let mut text = String::new();
    for segment in segments {
        if let Some(raw) = segment.as_str() {
            text.push_str(raw);
            continue;
        }
        let Some(segment_object) = segment.as_object() else {
            continue;
        };
        if let Some(data_text) = segment_object
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("text"))
            .and_then(Value::as_str)
        {
            text.push_str(data_text);
            continue;
        }
        if let Some(segment_text) = segment_object.get("text").and_then(Value::as_str) {
            text.push_str(segment_text);
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn declared_command_matches_message(command: &str, message: &str) -> bool {
    let Some(command) = normalize_tui_command_name(command) else {
        return false;
    };
    let message = message.trim();
    if matches_command_with_prefix(message, command.as_str()) {
        return true;
    }
    command
        .strip_prefix('/')
        .is_some_and(|bare| matches_command_with_prefix(message, bare))
}

fn matches_command_with_prefix(message: &str, command_prefix: &str) -> bool {
    let message = message.trim();
    if message.eq_ignore_ascii_case(command_prefix) {
        return true;
    }
    parse_command_argument(message, command_prefix).is_some()
}

fn parse_command_argument(message: &str, command_prefix: &str) -> Option<String> {
    let command_prefix = command_prefix.trim();
    if command_prefix.is_empty() {
        return None;
    }

    let message = message.trim();
    if message.eq_ignore_ascii_case(command_prefix) {
        return Some(String::new());
    }

    let remainder = message.strip_prefix(command_prefix)?;
    let mut chars = remainder.chars();
    if !chars.next().is_some_and(char::is_whitespace) {
        return None;
    }

    Some(remainder.trim().to_string())
}

pub(crate) fn normalize_tui_command_name(raw: &str) -> Option<String> {
    let first = raw.split_whitespace().next()?.trim();
    if first.is_empty() {
        return None;
    }
    let normalized = if first.starts_with('/') {
        first.to_ascii_lowercase()
    } else {
        format!("/{}", first.to_ascii_lowercase())
    };
    if normalized == "/" {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_plugin_scope(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let compact = trimmed.to_ascii_lowercase().replace([' ', '_', '-'], "");
    match compact.as_str() {
        "all" => Some("all".to_string()),
        "tui" => Some("tui".to_string()),
        "adapter:onebot11" | "adapter:onebotv11" | "adapteronebot11" | "onebot11" | "onebotv11" => {
            Some("adapter:onebot11".to_string())
        }
        _ => Some(trimmed.to_ascii_lowercase()),
    }
}

fn normalize_plugin_scopes(raw_scopes: &[String]) -> Vec<String> {
    let mut scopes = Vec::new();
    for scope in raw_scopes {
        if let Some(normalized) = normalize_plugin_scope(scope)
            && !scopes.iter().any(|existing| existing == &normalized)
        {
            scopes.push(normalized);
        }
    }
    if scopes.is_empty() {
        scopes.push("all".to_string());
    }
    scopes
}

fn plugin_scope_matches(scopes: &[String], scope: &str) -> bool {
    let Some(scope) = normalize_plugin_scope(scope) else {
        return false;
    };
    scopes
        .iter()
        .filter_map(|entry| normalize_plugin_scope(entry))
        .any(|entry| entry == "all" || entry == scope)
}

fn normalize_scoped_command_key(scope: &str, command: &str) -> Option<ScopedCommandKey> {
    Some(ScopedCommandKey {
        scope: normalize_plugin_scope(scope)?,
        command: normalize_tui_command_name(command)?,
    })
}

pub(crate) fn is_builtin_command_disabled_in_lock(
    lock: &PythonRuntimeState,
    scope: &str,
    command: &str,
) -> bool {
    normalize_scoped_command_key(scope, command)
        .is_some_and(|key| lock.disabled_scope_commands.contains(&key))
}

pub(crate) fn is_scope_command_disabled(
    state: &PythonRuntimeState,
    scope: &str,
    command: &str,
) -> bool {
    normalize_scoped_command_key(scope, command)
        .is_some_and(|key| state.disabled_scope_commands.contains(&key))
}

pub(crate) fn set_builtin_command_enabled_in_lock(
    lock: &mut PythonRuntimeState,
    scope: &str,
    command: &str,
    enabled: bool,
) -> Result<bool, String> {
    let Some(key) = normalize_scoped_command_key(scope, command) else {
        return Err("command scope or name is invalid".to_string());
    };
    if enabled {
        Ok(lock.disabled_scope_commands.remove(&key))
    } else {
        Ok(lock.disabled_scope_commands.insert(key))
    }
}

pub(crate) fn sync_disabled_scope_commands_in_lock(
    lock: &mut PythonRuntimeState,
    entries: &[String],
) -> Result<(), String> {
    let mut disabled = std::collections::HashSet::new();
    for entry in entries {
        let Some((scope, command)) = parse_disabled_scope_command_entry(entry.as_str()) else {
            return Err(format!(
                "invalid disabled scope command entry '{}': expected '<scope> <name>'",
                entry.trim()
            ));
        };
        let Some(key) = normalize_scoped_command_key(scope.as_str(), command.as_str()) else {
            return Err(format!(
                "invalid disabled scope command entry '{}': expected '<scope> <name>'",
                entry.trim()
            ));
        };
        disabled.insert(key);
    }
    lock.disabled_scope_commands = disabled;
    Ok(())
}

pub(crate) fn list_disabled_scope_commands(lock: &PythonRuntimeState) -> Vec<String> {
    let mut entries = lock
        .disabled_scope_commands
        .iter()
        .map(|entry| format!("{} {}", entry.scope, entry.command))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn parse_disabled_scope_command_entry(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    let (scope, command) = raw.split_once(char::is_whitespace)?;
    Some((scope.trim().to_string(), command.trim().to_string()))
}

pub(crate) fn register_declared_commands(
    commands: &mut Vec<PythonDeclaredCommandEntry>,
    plugin_id: &str,
    descriptors: &[PluginCommandDescriptor],
) {
    commands.retain(|command| command.plugin_id.as_str() != plugin_id);
    commands.extend(descriptors.iter().filter_map(|descriptor| {
        let command = normalize_tui_command_name(descriptor.name.as_str())?;
        let description = descriptor.description.trim();
        Some(PythonDeclaredCommandEntry {
            command,
            description: if description.is_empty() {
                "plugin declared command".to_string()
            } else {
                description.to_string()
            },
            plugin_id: plugin_id.to_string(),
            scopes: normalize_plugin_scopes(&descriptor.scopes),
        })
    }));
}

pub(crate) fn register_tui_command(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    command: &str,
    description: Option<String>,
    enabled: bool,
    handler: Py<PyAny>,
    py: Python<'_>,
) -> Result<(), String> {
    let Some(command) = normalize_tui_command_name(command) else {
        return Err("plugin command name should not be empty".to_string());
    };
    if !handler.bind(py).is_callable() {
        return Err(format!("handler for '{}' is not callable", command));
    }

    let description = description
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| "python plugin command".to_string());
    let mut lock = state
        .lock()
        .map_err(|_| "python runtime lock poisoned".to_string())?;
    if let Some(existing) = lock.commands.get(command.as_str())
        && existing.plugin_id != plugin_id
    {
        return Err(format!(
            "plugin command '{}' already registered by '{}'",
            command, existing.plugin_id
        ));
    }
    lock.commands.insert(
        command.clone(),
        PythonTuiCommandEntry {
            command: command.clone(),
            description,
            enabled,
            plugin_id: plugin_id.to_string(),
            handler,
        },
    );
    Ok(())
}

pub(crate) fn set_tui_command_enabled(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    command: &str,
    enabled: bool,
) -> Result<bool, String> {
    let Some(command) = normalize_tui_command_name(command) else {
        return Err("plugin command name should not be empty".to_string());
    };
    let mut lock = state
        .lock()
        .map_err(|_| "python runtime lock poisoned".to_string())?;
    if let Some(entry) = lock.commands.get_mut(command.as_str()) {
        if entry.plugin_id != plugin_id {
            return Err(format!(
                "plugin command '{}' belongs to '{}' and cannot be changed by '{}'",
                command, entry.plugin_id, plugin_id
            ));
        }
        entry.enabled = enabled;
        return Ok(true);
    }

    set_builtin_command_enabled_in_lock(&mut lock, "tui", command.as_str(), enabled)
}

pub(crate) fn set_scope_command_enabled(
    state: &Arc<Mutex<PythonRuntimeState>>,
    scope: &str,
    command: &str,
    enabled: bool,
) -> Result<usize, String> {
    let Some(scope) = normalize_plugin_scope(scope) else {
        return Err("command scope is invalid".to_string());
    };
    let Some(command) = normalize_tui_command_name(command) else {
        return Err("plugin command name should not be empty".to_string());
    };

    let mut lock = state
        .lock()
        .map_err(|_| "python runtime lock poisoned".to_string())?;
    let mut matched = lock.declared_commands.iter().any(|entry| {
        entry.command == command && plugin_scope_matches(&entry.scopes, scope.as_str())
    });

    if scope == "tui" {
        matched |= lock.commands.values().any(|entry| entry.command == command);
    }

    let changed =
        set_builtin_command_enabled_in_lock(&mut lock, scope.as_str(), command.as_str(), enabled)?;
    if matched || changed {
        Ok(usize::from(changed || matched))
    } else {
        Ok(0)
    }
}

pub(crate) fn remove_tui_command(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    command: &str,
) -> Result<bool, String> {
    let Some(command) = normalize_tui_command_name(command) else {
        return Err("plugin command name should not be empty".to_string());
    };
    let mut lock = state
        .lock()
        .map_err(|_| "python runtime lock poisoned".to_string())?;
    if let Some(entry) = lock.commands.get(command.as_str())
        && entry.plugin_id != plugin_id
    {
        return Err(format!(
            "plugin command '{}' belongs to '{}' and cannot be removed by '{}'",
            command, entry.plugin_id, plugin_id
        ));
    }
    Ok(lock.commands.remove(command.as_str()).is_some())
}

pub(crate) fn list_tui_commands(state: &Arc<Mutex<PythonRuntimeState>>) -> Vec<PluginTuiCommand> {
    let Ok(lock) = state.lock() else {
        return Vec::new();
    };
    let mut commands: Vec<PluginTuiCommand> = lock
        .commands
        .values()
        .map(|entry| PluginTuiCommand {
            name: entry.command.clone(),
            description: entry.description.clone(),
            enabled: entry.enabled
                && !is_scope_command_disabled(&lock, "tui", entry.command.as_str()),
            plugin_id: entry.plugin_id.clone(),
        })
        .collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    commands
}

pub(crate) fn list_scope_commands(
    state: &Arc<Mutex<PythonRuntimeState>>,
    scope: &str,
) -> Vec<PluginScopedCommand> {
    let Some(scope) = normalize_plugin_scope(scope) else {
        return Vec::new();
    };

    let Ok(lock) = state.lock() else {
        return Vec::new();
    };

    let mut merged: HashMap<String, PluginScopedCommand> = HashMap::new();

    for entry in &lock.declared_commands {
        if !plugin_scope_matches(&entry.scopes, scope.as_str()) {
            continue;
        }
        let key = format!("{}::{}", entry.plugin_id, entry.command);
        merged.insert(
            key,
            PluginScopedCommand {
                name: entry.command.clone(),
                description: entry.description.clone(),
                enabled: !is_scope_command_disabled(&lock, scope.as_str(), entry.command.as_str()),
                plugin_id: entry.plugin_id.clone(),
                scopes: entry.scopes.clone(),
                executable_in_tui: false,
            },
        );
    }

    for entry in lock.commands.values() {
        let scopes = vec!["tui".to_string()];
        if !plugin_scope_matches(&scopes, scope.as_str()) {
            continue;
        }
        let key = format!("{}::{}", entry.plugin_id, entry.command);
        merged
            .entry(key)
            .and_modify(|existing| {
                existing.description = entry.description.clone();
                existing.enabled = entry.enabled
                    && !is_scope_command_disabled(&lock, "tui", entry.command.as_str());
                existing.executable_in_tui = true;
                if !existing.scopes.iter().any(|scope| scope == "tui") {
                    existing.scopes.push("tui".to_string());
                }
            })
            .or_insert_with(|| PluginScopedCommand {
                name: entry.command.clone(),
                description: entry.description.clone(),
                enabled: entry.enabled
                    && !is_scope_command_disabled(&lock, "tui", entry.command.as_str()),
                plugin_id: entry.plugin_id.clone(),
                scopes,
                executable_in_tui: true,
            });
    }

    let mut commands: Vec<PluginScopedCommand> = merged.into_values().collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name).then(a.plugin_id.cmp(&b.plugin_id)));
    commands
}
