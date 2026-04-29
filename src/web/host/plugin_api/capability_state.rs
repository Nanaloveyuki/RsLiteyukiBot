#[path = "capability_state/payloads.rs"]
mod payloads;
#[path = "capability_state/support.rs"]
mod support;

use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginCapabilitySupportState {
    registered: bool,
    executable: bool,
    persistent: bool,
    active: bool,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginCapabilitySupportSummary {
    pub(super) tools: PluginCapabilitySupportState,
    pub(super) web_apis: PluginCapabilitySupportState,
    pub(super) cron_jobs: PluginCapabilitySupportState,
    pub(super) tasks: PluginCapabilitySupportState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginCapabilitiesPayload {
    pub(super) plugin_id: String,
    pub(super) runtime_kind: crate::PluginRuntimeKind,
    pub(super) support: PluginCapabilitySupportSummary,
    pub(super) snapshot: crate::PluginCapabilitySnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginRuntimeBindingSummary {
    tools: bool,
    web_apis: bool,
    cron_jobs: bool,
    tasks: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginRuntimeStatePayload {
    plugin_id: String,
    runtime_kind: crate::PluginRuntimeKind,
    loaded: bool,
    enabled: bool,
    active: bool,
    snapshot_extracted: bool,
    executable_bindings: PluginRuntimeBindingSummary,
    scheduler_status: String,
    task_runtime_status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginDiagnosticsPayload {
    plugin_id: String,
    runtime_kind: crate::PluginRuntimeKind,
    load_state: String,
    snapshot_extracted: bool,
    executable_bindings: PluginRuntimeBindingSummary,
    scheduler_status: String,
    last_web_api_dispatch: crate::PluginExecutionRecord,
    last_tool_execution: crate::PluginExecutionRecord,
    last_cron_execution: crate::PluginExecutionRecord,
}

pub(super) fn plugin_capabilities_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginCapabilitiesPayload, String> {
    payloads::plugin_capabilities_payload(service, plugin_id)
}

pub(super) fn all_plugin_capabilities_payload(
    service: &WebHostService,
) -> Result<Vec<PluginCapabilitiesPayload>, String> {
    payloads::all_plugin_capabilities_payload(service)
}

pub(super) fn plugin_runtime_state_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginRuntimeStatePayload, String> {
    payloads::plugin_runtime_state_payload(service, plugin_id)
}

pub(super) fn plugin_diagnostics_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginDiagnosticsPayload, String> {
    payloads::plugin_diagnostics_payload(service, plugin_id)
}
