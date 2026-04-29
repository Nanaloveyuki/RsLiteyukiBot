use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    AdapterConfig, LiteyukiBot, LlmFunctionTool, PluginCapabilitySnapshot,
    PluginRuntimeDiagnostics, PluginToolResult, PluginWebApiRequest, PluginWebApiResponse,
};

#[path = "app_host/resource_usage.rs"]
mod resource_usage;
#[path = "app_host/runtime.rs"]
mod runtime;
#[path = "app_host/state.rs"]
mod state;
#[cfg(test)]
#[path = "app_host/tests.rs"]
mod tests;

pub use self::resource_usage::{AppHostCpuUsage, AppHostMemoryUsage, AppHostResourceUsage};
pub(crate) use self::state::{
    APP_TITLE, AppHostState, AppHostStateSnapshot, runtime_target_name, with_state_write,
};
pub use self::state::{AppHostExternalStats, AppHostPluginCatalogSnapshot, AppHostSnapshot};

#[derive(Clone)]
pub struct EmbeddedAppHost {
    bot: Arc<AsyncMutex<LiteyukiBot>>,
    state: Arc<RwLock<AppHostState>>,
}

impl EmbeddedAppHost {
    pub fn snapshot(&self) -> AppHostSnapshot {
        self.state
            .read()
            .expect("embedded app host state lock should not be poisoned")
            .snapshot()
    }

    pub async fn plugin_catalog_snapshot(&self) -> AppHostPluginCatalogSnapshot {
        let bot = self.bot.lock().await;
        AppHostPluginCatalogSnapshot {
            entries: bot.plugin_manager().plugin_catalog(),
            disabled_plugin_ids: bot.disabled_plugin_ids(),
        }
    }

    pub async fn plugin_capability_snapshot(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginCapabilitySnapshot>, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .get_plugin_capabilities(plugin_id)
            .map_err(|err| err.to_string())
    }

    pub async fn all_plugin_capability_snapshots(
        &self,
    ) -> Result<Vec<PluginCapabilitySnapshot>, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .list_all_plugin_capabilities()
            .map_err(|err| err.to_string())
    }

    pub async fn dispatch_plugin_web_api(
        &self,
        plugin_id: &str,
        route: &str,
        request: &PluginWebApiRequest,
    ) -> Result<Option<PluginWebApiResponse>, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .execute_plugin_web_api(plugin_id, route, request)
            .map_err(|err| err.to_string())
    }

    pub async fn execute_plugin_tool(
        &self,
        plugin_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<Option<PluginToolResult>, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .execute_plugin_tool(plugin_id, tool_name, arguments)
            .map_err(|err| err.to_string())
    }

    pub async fn build_all_plugin_tool_bundle(&self) -> Result<Vec<LlmFunctionTool>, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .build_all_plugin_tool_bundle()
            .map_err(|err| err.to_string())
    }

    pub async fn plugin_runtime_diagnostics(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginRuntimeDiagnostics>, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .get_plugin_runtime_diagnostics(plugin_id)
            .map_err(|err| err.to_string())
    }

    pub async fn plugin_cron_scheduler_status(&self, plugin_id: &str) -> Result<String, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .plugin_cron_scheduler_status(plugin_id)
            .map_err(|err| err.to_string())
    }

    pub async fn plugin_has_executable_cron_jobs(&self, plugin_id: &str) -> Result<bool, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .plugin_has_executable_cron_jobs(plugin_id)
            .map_err(|err| err.to_string())
    }

    pub async fn run_plugin_cron_tick(&self) -> Result<usize, String> {
        self.run_plugin_cron_tick_at(Utc::now()).await
    }

    pub async fn run_plugin_cron_tick_at(&self, now: DateTime<Utc>) -> Result<usize, String> {
        let bot = self.bot.lock().await;
        bot.plugin_sdk()
            .run_due_plugin_jobs(bot.disabled_plugin_ids().as_slice(), Some(now))
            .map_err(|err| err.to_string())
    }

    pub async fn adapter_configs(&self) -> Vec<AdapterConfig> {
        let bot = self.bot.lock().await;
        bot.adapter_manager().list()
    }

    pub async fn apply_disabled_plugins(
        &self,
        disabled_plugin_ids: Vec<String>,
    ) -> Result<(), String> {
        runtime::apply_disabled_plugins(self, disabled_plugin_ids).await
    }

    pub async fn apply_adapter_configs(
        &self,
        adapter_configs: Vec<AdapterConfig>,
    ) -> Result<(), String> {
        runtime::apply_adapter_configs(self, adapter_configs).await
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        runtime::shutdown_embedded_app_host(self).await
    }
}
