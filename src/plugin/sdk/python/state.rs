use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::plugin::PluginRuntimeDiagnostics;

use super::bridge::PyPluginSdk;

pub(crate) struct PythonLoadedPlugin {
    pub(crate) event_handler: Option<Py<PyAny>>,
    pub(crate) start_handler: Option<Py<PyAny>>,
    pub(crate) health_handler: Option<Py<PyAny>>,
    pub(crate) shutdown_handler: Option<Py<PyAny>>,
    pub(crate) unload_handler: Option<Py<PyAny>>,
    pub(crate) sdk: Py<PyPluginSdk>,
    pub(crate) runtime_module: String,
    pub(crate) module_names: Vec<String>,
    pub(crate) search_paths: Vec<PathBuf>,
}

pub(crate) struct PythonTuiCommandEntry {
    pub(crate) command: String,
    pub(crate) description: String,
    pub(crate) enabled: bool,
    pub(crate) plugin_id: String,
    pub(crate) handler: Py<PyAny>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ScopedCommandKey {
    pub(crate) scope: String,
    pub(crate) command: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PythonDeclaredCommandEntry {
    pub(crate) command: String,
    pub(crate) description: String,
    pub(crate) plugin_id: String,
    pub(crate) scopes: Vec<String>,
}

#[derive(Default)]
pub(crate) struct PythonRuntimeState {
    pub(crate) plugins: std::collections::HashMap<String, PythonLoadedPlugin>,
    pub(crate) commands: std::collections::HashMap<String, PythonTuiCommandEntry>,
    pub(crate) declared_commands: Vec<PythonDeclaredCommandEntry>,
    pub(crate) disabled_scope_commands: std::collections::HashSet<ScopedCommandKey>,
    pub(crate) diagnostics: std::collections::HashMap<String, PluginRuntimeDiagnostics>,
}

pub(super) fn remove_plugin_runtime_state(state: &mut PythonRuntimeState, plugin_id: &str) {
    state.plugins.remove(plugin_id);
    state.diagnostics.remove(plugin_id);
    state
        .commands
        .retain(|_, command| command.plugin_id.as_str() != plugin_id);
    state
        .declared_commands
        .retain(|command| command.plugin_id.as_str() != plugin_id);
}

pub(super) fn other_plugins_use_search_path(
    state: &PythonRuntimeState,
    plugin_id: &str,
    path: &PathBuf,
) -> bool {
    state.plugins.iter().any(|(other_id, plugin)| {
        other_id != plugin_id
            && plugin
                .search_paths
                .iter()
                .any(|candidate| candidate == path)
    })
}
