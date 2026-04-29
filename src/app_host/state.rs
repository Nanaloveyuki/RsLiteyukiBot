use std::sync::{Arc, RwLock};

use serde::Serialize;

use crate::runtime_support::ExternalGatewaySnapshot;
use crate::{PluginCatalogEntry, RuntimeTarget};

pub(crate) const APP_TITLE: &str = "Liteyuki";
const MAX_STATUS_NOTES: usize = 32;

#[derive(Debug, Clone)]
pub struct AppHostPluginCatalogSnapshot {
    pub entries: Vec<PluginCatalogEntry>,
    pub disabled_plugin_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppHostExternalStats {
    pub command_hits: u64,
    pub api_requests: u64,
    pub api_success: u64,
    pub api_failed: u64,
    pub api_timeouts: u64,
    pub api_inflight: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppHostSnapshot {
    pub app_name: String,
    pub status: String,
    pub runtime_target: String,
    pub locale: String,
    pub runtime_config: String,
    pub adapter_count: usize,
    pub adapter_autostart: bool,
    pub plugin_dirs: Vec<String>,
    pub disabled_commands: Vec<String>,
    pub disabled_plugins: Vec<String>,
    pub llm_command_prefix: String,
    pub help_whitelist_size: usize,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
    pub last_event_topic: Option<String>,
    pub last_event_preview: Option<String>,
    pub handled_events: u64,
    pub external_stats: AppHostExternalStats,
    pub resource_usage: super::AppHostResourceUsage,
}

impl Default for AppHostSnapshot {
    fn default() -> Self {
        Self {
            app_name: APP_TITLE.to_string(),
            status: "initializing".to_string(),
            runtime_target: "unknown".to_string(),
            locale: "zh-CN".to_string(),
            runtime_config: String::new(),
            adapter_count: 0,
            adapter_autostart: false,
            plugin_dirs: Vec::new(),
            disabled_commands: Vec::new(),
            disabled_plugins: Vec::new(),
            llm_command_prefix: "/ask".to_string(),
            help_whitelist_size: 0,
            warnings: Vec::new(),
            notes: Vec::new(),
            last_event_topic: None,
            last_event_preview: None,
            handled_events: 0,
            external_stats: AppHostExternalStats::default(),
            resource_usage: super::AppHostResourceUsage::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AppHostStateSnapshot {
    pub app_name: String,
    pub status: String,
    pub runtime_target: String,
    pub locale: String,
    pub runtime_config: String,
    pub adapter_count: usize,
    pub adapter_autostart: bool,
    pub plugin_dirs: Vec<String>,
    pub disabled_commands: Vec<String>,
    pub disabled_plugins: Vec<String>,
    pub llm_command_prefix: String,
    pub help_whitelist_size: usize,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
    pub last_event_topic: Option<String>,
    pub last_event_preview: Option<String>,
    pub handled_events: u64,
    pub external_stats: AppHostExternalStats,
    pub resource_usage: super::AppHostResourceUsage,
}

impl Default for AppHostStateSnapshot {
    fn default() -> Self {
        Self {
            app_name: APP_TITLE.to_string(),
            status: "initializing".to_string(),
            runtime_target: "unknown".to_string(),
            locale: "zh-CN".to_string(),
            runtime_config: String::new(),
            adapter_count: 0,
            adapter_autostart: false,
            plugin_dirs: Vec::new(),
            disabled_commands: Vec::new(),
            disabled_plugins: Vec::new(),
            llm_command_prefix: "/ask".to_string(),
            help_whitelist_size: 0,
            warnings: Vec::new(),
            notes: Vec::new(),
            last_event_topic: None,
            last_event_preview: None,
            handled_events: 0,
            external_stats: AppHostExternalStats::default(),
            resource_usage: super::AppHostResourceUsage::default(),
        }
    }
}

impl From<&AppHostStateSnapshot> for AppHostSnapshot {
    fn from(snapshot: &AppHostStateSnapshot) -> Self {
        Self {
            app_name: snapshot.app_name.clone(),
            status: snapshot.status.clone(),
            runtime_target: snapshot.runtime_target.clone(),
            locale: snapshot.locale.clone(),
            runtime_config: snapshot.runtime_config.clone(),
            adapter_count: snapshot.adapter_count,
            adapter_autostart: snapshot.adapter_autostart,
            plugin_dirs: snapshot.plugin_dirs.clone(),
            disabled_commands: snapshot.disabled_commands.clone(),
            disabled_plugins: snapshot.disabled_plugins.clone(),
            llm_command_prefix: snapshot.llm_command_prefix.clone(),
            help_whitelist_size: snapshot.help_whitelist_size,
            warnings: snapshot.warnings.clone(),
            notes: snapshot.notes.clone(),
            last_event_topic: snapshot.last_event_topic.clone(),
            last_event_preview: snapshot.last_event_preview.clone(),
            handled_events: snapshot.handled_events,
            external_stats: snapshot.external_stats.clone(),
            resource_usage: snapshot.resource_usage.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct AppHostState {
    pub(crate) snapshot: AppHostStateSnapshot,
}

impl AppHostState {
    pub(crate) fn new(snapshot: AppHostStateSnapshot) -> Self {
        Self { snapshot }
    }

    pub(crate) fn snapshot(&self) -> AppHostSnapshot {
        AppHostSnapshot::from(&self.snapshot)
    }

    pub(crate) fn set_status(&mut self, status: impl Into<String>) {
        self.snapshot.status = status.into();
    }

    pub(crate) fn push_note(&mut self, note: impl Into<String>) {
        self.snapshot.notes.push(note.into());
        if self.snapshot.notes.len() > MAX_STATUS_NOTES {
            let overflow = self.snapshot.notes.len() - MAX_STATUS_NOTES;
            self.snapshot.notes.drain(0..overflow);
        }
    }

    pub(crate) fn push_warning(&mut self, warning: impl Into<String>) {
        let warning = warning.into();
        if !self.snapshot.warnings.iter().any(|entry| entry == &warning) {
            self.snapshot.warnings.push(warning);
        }
    }

    pub(crate) fn set_external_stats(&mut self, snapshot: &ExternalGatewaySnapshot) {
        self.snapshot.external_stats = AppHostExternalStats {
            command_hits: snapshot.command_hits,
            api_requests: snapshot.api_requests,
            api_success: snapshot.api_success,
            api_failed: snapshot.api_failed,
            api_timeouts: snapshot.api_timeouts,
            api_inflight: snapshot.api_inflight as u64,
        };
    }

    pub(crate) fn set_resource_usage(&mut self, resource_usage: super::AppHostResourceUsage) {
        self.snapshot.resource_usage = resource_usage;
    }

    pub(crate) fn record_event(&mut self, topic: String, preview: String) {
        self.snapshot.last_event_topic = Some(topic);
        self.snapshot.last_event_preview = Some(preview);
        self.snapshot.handled_events = self.snapshot.handled_events.saturating_add(1);
    }
}

pub(crate) fn with_state_write(
    state: &Arc<RwLock<AppHostState>>,
    updater: impl FnOnce(&mut AppHostState),
) {
    if let Ok(mut lock) = state.write() {
        updater(&mut lock);
    }
}

pub(crate) fn runtime_target_name(target: RuntimeTarget) -> &'static str {
    match target {
        RuntimeTarget::Cli => "cli",
        RuntimeTarget::Web => "web",
        RuntimeTarget::Tauri2 => "tauri2",
        RuntimeTarget::Docker => "docker",
        RuntimeTarget::CliWeb => "cli-web",
        RuntimeTarget::DockerWeb => "docker-web",
    }
}
