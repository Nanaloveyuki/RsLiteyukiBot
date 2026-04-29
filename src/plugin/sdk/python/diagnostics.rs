use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::plugin::sdk::python::state::PythonRuntimeState;
use crate::plugin::{PluginExecutionRecord, PluginRuntimeDiagnostics};

#[derive(Debug, Clone, Copy)]
pub(super) enum PluginExecutionKind {
    WebApi,
    Tool,
    Cron,
}

pub(super) fn record_plugin_execution_success(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    kind: PluginExecutionKind,
) {
    if let Ok(mut lock) = state.lock() {
        let diagnostics = lock
            .diagnostics
            .entry(plugin_id.to_string())
            .or_insert_with(|| PluginRuntimeDiagnostics {
                plugin_id: plugin_id.to_string(),
                ..PluginRuntimeDiagnostics::default()
            });
        let record = execution_record_mut(diagnostics, kind);
        record.last_success_at = Some(Utc::now().to_rfc3339());
    }
}

pub(super) fn record_plugin_execution_error(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    kind: PluginExecutionKind,
    error: String,
) {
    if let Ok(mut lock) = state.lock() {
        let diagnostics = lock
            .diagnostics
            .entry(plugin_id.to_string())
            .or_insert_with(|| PluginRuntimeDiagnostics {
                plugin_id: plugin_id.to_string(),
                ..PluginRuntimeDiagnostics::default()
            });
        let record = execution_record_mut(diagnostics, kind);
        record.last_error = Some(error);
        record.last_error_at = Some(Utc::now().to_rfc3339());
    }
}

fn execution_record_mut(
    diagnostics: &mut PluginRuntimeDiagnostics,
    kind: PluginExecutionKind,
) -> &mut PluginExecutionRecord {
    match kind {
        PluginExecutionKind::WebApi => &mut diagnostics.last_web_api_dispatch,
        PluginExecutionKind::Tool => &mut diagnostics.last_tool_execution,
        PluginExecutionKind::Cron => &mut diagnostics.last_cron_execution,
    }
}
