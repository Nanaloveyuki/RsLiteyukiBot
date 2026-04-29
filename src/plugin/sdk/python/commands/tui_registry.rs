use std::sync::{Arc, Mutex};

use pyo3::prelude::*;
use pyo3::types::PyAny;

use super::super::state::{PythonRuntimeState, PythonTuiCommandEntry};
use super::models::PluginTuiCommand;
use super::normalization::normalize_tui_command_name;
use super::scope_state::{is_scope_command_disabled, set_builtin_command_enabled_in_lock};

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
