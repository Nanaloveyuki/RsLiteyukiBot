use std::sync::{Arc, Mutex};

use chrono::Utc;
use pyo3::prelude::*;

use crate::plugin::sdk::PluginSdkError;
use crate::plugin::sdk::python::runtime_registry::fetch_python_plugin_capability_snapshot_payload;
use crate::plugin::sdk::python::state::PythonRuntimeState;
use crate::plugin::{
    PluginCapabilitySnapshot, PluginRegisteredCronJob, PluginRegisteredTask, PluginRegisteredTool,
    PluginRegisteredWebApi, PluginRuntimeDiagnostics,
};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonCapabilitySnapshotDoc {
    #[serde(default)]
    tools: Vec<PluginRegisteredTool>,
    #[serde(default)]
    web_apis: Vec<PluginRegisteredWebApi>,
    #[serde(default)]
    cron_jobs: Vec<PluginRegisteredCronJob>,
    #[serde(default)]
    tasks: Vec<PluginRegisteredTask>,
}

pub(crate) fn get_python_plugin_capability_snapshot(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<Option<PluginCapabilitySnapshot>, PluginSdkError> {
    let runtime_module = {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(None);
        };
        plugin.runtime_module.clone()
    };

    let payload = Python::with_gil(|py| {
        fetch_python_plugin_capability_snapshot_payload(py, plugin_id, runtime_module.as_str())
    })?;

    let Some(payload) = payload else {
        return Ok(None);
    };
    let mut document: PythonCapabilitySnapshotDoc =
        serde_json::from_value(payload).map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' capability snapshot decode failed: {}",
                plugin_id, err
            ))
        })?;
    populate_snapshot_plugin_ids(&mut document, plugin_id);

    Ok(Some(PluginCapabilitySnapshot {
        plugin_id: plugin_id.to_string(),
        runtime_kind: crate::plugin::PluginRuntimeKind::Python,
        tools: document.tools,
        web_apis: document.web_apis,
        cron_jobs: document.cron_jobs,
        tasks: document.tasks,
        updated_at: Utc::now().to_rfc3339(),
    }))
}

pub(crate) fn list_all_python_plugin_capability_snapshots(
    state: &Arc<Mutex<PythonRuntimeState>>,
) -> Result<Vec<PluginCapabilitySnapshot>, PluginSdkError> {
    let mut plugin_ids = {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        lock.plugins.keys().cloned().collect::<Vec<_>>()
    };
    plugin_ids.sort();

    let mut snapshots = Vec::with_capacity(plugin_ids.len());
    for plugin_id in plugin_ids {
        if let Some(snapshot) = get_python_plugin_capability_snapshot(state, plugin_id.as_str())? {
            snapshots.push(snapshot);
        }
    }
    Ok(snapshots)
}

pub(crate) fn get_python_plugin_runtime_diagnostics(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<Option<PluginRuntimeDiagnostics>, PluginSdkError> {
    let lock = state
        .lock()
        .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
    Ok(lock.diagnostics.get(plugin_id).cloned())
}

fn populate_snapshot_plugin_ids(document: &mut PythonCapabilitySnapshotDoc, plugin_id: &str) {
    for tool in &mut document.tools {
        if tool.plugin_id.is_empty() {
            tool.plugin_id = plugin_id.to_string();
        }
    }
    for web_api in &mut document.web_apis {
        if web_api.plugin_id.is_empty() {
            web_api.plugin_id = plugin_id.to_string();
        }
    }
    for cron_job in &mut document.cron_jobs {
        if cron_job.plugin_id.is_empty() {
            cron_job.plugin_id = plugin_id.to_string();
        }
    }
    for task in &mut document.tasks {
        if task.plugin_id.is_empty() {
            task.plugin_id = plugin_id.to_string();
        }
    }
}

#[cfg(test)]
#[path = "runtime_introspection/tests.rs"]
mod tests;
