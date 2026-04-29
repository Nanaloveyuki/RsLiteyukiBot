use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::super::state::PythonRuntimeState;
use super::models::PluginScopedCommand;
use super::normalization::{
    normalize_plugin_scope, normalize_scoped_command_key, normalize_tui_command_name,
    plugin_scope_matches,
};

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

fn parse_disabled_scope_command_entry(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    let (scope, command) = raw.split_once(char::is_whitespace)?;
    Some((scope.trim().to_string(), command.trim().to_string()))
}
