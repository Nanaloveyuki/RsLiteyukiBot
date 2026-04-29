use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::llm::{LlmClientError, LlmFunctionTool, LlmToolOutput};
use crate::plugin::{
    PluginCapabilitySnapshot, PluginRegisteredCronJob, PluginRegisteredTask, PluginRegisteredTool,
    PluginRegisteredWebApi, PluginRuntimeDiagnostics,
};

use super::python::runtime_introspection::{
    get_python_plugin_capability_snapshot, get_python_plugin_runtime_diagnostics,
    list_all_python_plugin_capability_snapshots,
};
use super::{PluginSdk, PluginSdkError, PluginToolResult, plugin_runtime_tool_name};

impl PluginSdk {
    pub fn get_plugin_capabilities(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginCapabilitySnapshot>, PluginSdkError> {
        let mut snapshot = self.get_plugin_capabilities_raw(plugin_id)?;
        if let Some(snapshot) = snapshot.as_mut() {
            self.sync_plugin_cron_snapshot(snapshot, Utc::now())?;
        }
        Ok(snapshot)
    }

    pub fn list_plugin_tools(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginRegisteredTool>, PluginSdkError> {
        Ok(self
            .get_plugin_capabilities(plugin_id)?
            .map(|snapshot| snapshot.tools)
            .unwrap_or_default())
    }

    pub fn list_plugin_web_apis(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginRegisteredWebApi>, PluginSdkError> {
        Ok(self
            .get_plugin_capabilities(plugin_id)?
            .map(|snapshot| snapshot.web_apis)
            .unwrap_or_default())
    }

    pub fn list_plugin_cron_jobs(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginRegisteredCronJob>, PluginSdkError> {
        Ok(self
            .get_plugin_capabilities(plugin_id)?
            .map(|snapshot| snapshot.cron_jobs)
            .unwrap_or_default())
    }

    pub fn list_plugin_tasks(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginRegisteredTask>, PluginSdkError> {
        Ok(self
            .get_plugin_capabilities(plugin_id)?
            .map(|snapshot| snapshot.tasks)
            .unwrap_or_default())
    }

    pub fn list_all_plugin_capabilities(
        &self,
    ) -> Result<Vec<PluginCapabilitySnapshot>, PluginSdkError> {
        let mut snapshots = self.list_all_plugin_capabilities_raw()?;
        self.sync_all_plugin_cron_snapshots(snapshots.as_mut_slice(), true, Utc::now())?;
        Ok(snapshots)
    }

    pub fn build_plugin_tool_bundle(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<LlmFunctionTool>, PluginSdkError> {
        let tools = self.list_plugin_tools(plugin_id)?;
        let mut bundle = Vec::new();
        for tool in tools.into_iter().filter(|tool| tool.active) {
            let runtime_name = plugin_runtime_tool_name(plugin_id, tool.name.as_str());
            let original_name = tool.name.clone();
            let description = tool.description.clone();
            let parameters = tool.parameters.clone();
            let sdk = self.clone();
            let plugin_id = plugin_id.to_string();
            let runtime_name_for_handler = runtime_name.clone();
            let original_name_for_handler = original_name.clone();
            let llm_tool = LlmFunctionTool::new(runtime_name, parameters, move |arguments| {
                let sdk = sdk.clone();
                let plugin_id = plugin_id.clone();
                let runtime_name = runtime_name_for_handler.clone();
                let original_name = original_name_for_handler.clone();
                async move {
                    match sdk.execute_plugin_tool(&plugin_id, &original_name, &arguments) {
                        Ok(Some(PluginToolResult::Text(text))) => Ok(LlmToolOutput::Text(text)),
                        Ok(Some(PluginToolResult::Json(value))) => Ok(LlmToolOutput::Json(value)),
                        Ok(None) => Err(LlmClientError::Tool(format!(
                            "plugin tool '{}' is unavailable",
                            runtime_name
                        ))),
                        Err(err) => Err(LlmClientError::Tool(err.to_string())),
                    }
                }
            })
            .with_description(description);
            bundle.push(llm_tool);
        }
        Ok(bundle)
    }

    pub fn build_all_plugin_tool_bundle(&self) -> Result<Vec<LlmFunctionTool>, PluginSdkError> {
        let snapshots = self.list_all_plugin_capabilities()?;
        let mut bundle = Vec::new();
        let mut seen_names = HashSet::new();
        for snapshot in snapshots {
            for tool in self.build_plugin_tool_bundle(snapshot.plugin_id.as_str())? {
                if !seen_names.insert(tool.name.clone()) {
                    return Err(PluginSdkError::Runtime(format!(
                        "duplicate plugin runtime tool name '{}'",
                        tool.name
                    )));
                }
                bundle.push(tool);
            }
        }
        Ok(bundle)
    }

    pub fn get_plugin_runtime_diagnostics(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginRuntimeDiagnostics>, PluginSdkError> {
        get_python_plugin_runtime_diagnostics(&self.python_runtime, plugin_id)
    }

    pub(super) fn get_plugin_capabilities_raw(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginCapabilitySnapshot>, PluginSdkError> {
        get_python_plugin_capability_snapshot(&self.python_runtime, plugin_id)
    }

    pub(super) fn list_all_plugin_capabilities_raw(
        &self,
    ) -> Result<Vec<PluginCapabilitySnapshot>, PluginSdkError> {
        list_all_python_plugin_capability_snapshots(&self.python_runtime)
    }

    pub(super) fn sync_plugin_cron_snapshot(
        &self,
        snapshot: &mut PluginCapabilitySnapshot,
        now: DateTime<Utc>,
    ) -> Result<(), PluginSdkError> {
        self.cron_scheduler
            .lock()
            .map_err(|_| {
                PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
            })?
            .sync_snapshot(snapshot, now)
            .map_err(PluginSdkError::Runtime)
    }

    pub(super) fn sync_all_plugin_cron_snapshots(
        &self,
        snapshots: &mut [PluginCapabilitySnapshot],
        prune_missing: bool,
        now: DateTime<Utc>,
    ) -> Result<(), PluginSdkError> {
        self.cron_scheduler
            .lock()
            .map_err(|_| {
                PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
            })?
            .sync_snapshots(snapshots, prune_missing, now)
            .map_err(PluginSdkError::Runtime)
    }
}
