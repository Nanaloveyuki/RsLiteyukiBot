use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid};
use tokio::sync::Mutex as AsyncMutex;

use crate::external_commands::{ExternalCommandObserver, install_external_event_handlers};
use crate::i18n::{tr, trf};
use crate::onebot_support::{payload_preview, should_hide_event_from_tui};
use crate::runtime_support::{
    EXTERNAL_API_TIMEOUT, ExternalGatewaySnapshot, describe_runtime_config,
    prepare_runtime_bootstrap,
};
#[cfg(test)]
use crate::runtime_support::{
    dedup_warnings, push_explicit_plugin_dir_candidates, push_runtime_plugin_dir_candidates,
};
use crate::{
    AdapterConfig, LiteyukiBot, LlmFunctionTool, LogLevel, PluginCapabilitySnapshot,
    PluginCatalogEntry, PluginRuntimeDiagnostics, PluginToolResult, PluginWebApiRequest,
    PluginWebApiResponse, RuntimeTarget, emit_console_log,
};
#[cfg(test)]
use std::collections::HashSet;
#[cfg(test)]
use std::path::PathBuf;

const APP_TITLE: &str = "Liteyuki";
const RESOURCE_USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const PLUGIN_CRON_SAMPLE_INTERVAL: Duration = Duration::from_secs(15);
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

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppHostCpuUsage {
    pub system_percent: f32,
    pub process_percent: f32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppHostMemoryUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub process_bytes: u64,
    pub system_percent: f32,
    pub process_percent: f32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppHostResourceUsage {
    pub cpu: AppHostCpuUsage,
    pub memory: AppHostMemoryUsage,
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
    pub resource_usage: AppHostResourceUsage,
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
            resource_usage: AppHostResourceUsage::default(),
        }
    }
}

#[derive(Debug, Clone)]
struct AppHostStateSnapshot {
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
    pub resource_usage: AppHostResourceUsage,
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
            resource_usage: AppHostResourceUsage::default(),
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
struct AppHostState {
    snapshot: AppHostStateSnapshot,
}

impl AppHostState {
    fn new(snapshot: AppHostStateSnapshot) -> Self {
        Self { snapshot }
    }

    fn snapshot(&self) -> AppHostSnapshot {
        AppHostSnapshot::from(&self.snapshot)
    }

    fn set_status(&mut self, status: impl Into<String>) {
        self.snapshot.status = status.into();
    }

    fn push_note(&mut self, note: impl Into<String>) {
        self.snapshot.notes.push(note.into());
        if self.snapshot.notes.len() > MAX_STATUS_NOTES {
            let overflow = self.snapshot.notes.len() - MAX_STATUS_NOTES;
            self.snapshot.notes.drain(0..overflow);
        }
    }

    fn push_warning(&mut self, warning: impl Into<String>) {
        let warning = warning.into();
        if !self.snapshot.warnings.iter().any(|entry| entry == &warning) {
            self.snapshot.warnings.push(warning);
        }
    }

    fn set_external_stats(&mut self, snapshot: &ExternalGatewaySnapshot) {
        self.snapshot.external_stats = AppHostExternalStats {
            command_hits: snapshot.command_hits,
            api_requests: snapshot.api_requests,
            api_success: snapshot.api_success,
            api_failed: snapshot.api_failed,
            api_timeouts: snapshot.api_timeouts,
            api_inflight: snapshot.api_inflight as u64,
        };
    }

    fn set_resource_usage(&mut self, resource_usage: AppHostResourceUsage) {
        self.snapshot.resource_usage = resource_usage;
    }

    fn record_event(&mut self, topic: String, preview: String) {
        self.snapshot.last_event_topic = Some(topic);
        self.snapshot.last_event_preview = Some(preview);
        self.snapshot.handled_events = self.snapshot.handled_events.saturating_add(1);
    }
}

#[derive(Clone)]
pub struct EmbeddedAppHost {
    bot: Arc<AsyncMutex<LiteyukiBot>>,
    state: Arc<RwLock<AppHostState>>,
}

impl EmbeddedAppHost {
    pub async fn start_for_target(target: RuntimeTarget) -> Result<Self, String> {
        let bootstrap = prepare_runtime_bootstrap(target, |warning| {
            emit_console_log(LogLevel::Warn, "app.host", warning);
        })?;
        let effective_runtime_config = bootstrap.effective_runtime_config.clone();
        let runtime_config = bootstrap.runtime_config;
        let adapter_configs = bootstrap.adapter_configs;
        let adapter_autostart = bootstrap.adapter_autostart;
        let help_whitelist = bootstrap.help_whitelist;
        let locale = bootstrap.locale;
        let llm_runtime = bootstrap.llm_runtime;
        let external_gateway = bootstrap.external_gateway;
        let plugin_dirs = bootstrap.plugin_dirs;
        let disabled_commands = bootstrap.disabled_commands;
        let disabled_plugins = bootstrap.disabled_plugins;
        let superuser_manager = bootstrap.superuser_manager;
        let warnings = bootstrap.warnings;

        let state = Arc::new(RwLock::new(AppHostState::new(AppHostStateSnapshot {
            app_name: APP_TITLE.to_string(),
            status: "starting".to_string(),
            runtime_target: runtime_target_name(target).to_string(),
            locale,
            runtime_config: describe_runtime_config(&effective_runtime_config),
            adapter_count: adapter_configs.len(),
            adapter_autostart,
            plugin_dirs: plugin_dirs
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            disabled_commands: disabled_commands.clone(),
            disabled_plugins: disabled_plugins.clone(),
            llm_command_prefix: llm_runtime.command_prefix(),
            help_whitelist_size: help_whitelist
                .read()
                .map(|entries| entries.len())
                .unwrap_or_default(),
            warnings,
            notes: Vec::new(),
            last_event_topic: None,
            last_event_preview: None,
            handled_events: 0,
            external_stats: AppHostExternalStats::default(),
            resource_usage: AppHostResourceUsage::default(),
        })));
        with_state_write(&state, |host| {
            host.push_note(format!(
                "bootstrapping target={} adapters={} plugin_dirs={}",
                runtime_target_name(target),
                adapter_configs.len(),
                plugin_dirs.len()
            ));
        });

        let state_for_handler = state.clone();
        let external_gateway_for_handler = external_gateway.clone();
        let mut bot = LiteyukiBot::builder(APP_TITLE, env!("CARGO_PKG_VERSION"))
            .with_target(target)
            .with_runtime_config(runtime_config)
            .with_adapter_configs(adapter_configs)
            .with_adapter_autostart(false)
            .with_plugin_dirs(plugin_dirs.clone())
            .with_event_handler(move |event, _logger| {
                let state = state_for_handler.clone();
                let gateway = external_gateway_for_handler.clone();
                async move {
                    let snapshot = gateway.observe_payload(&event.payload, EXTERNAL_API_TIMEOUT);
                    with_state_write(&state, |host| host.set_external_stats(&snapshot));
                    if should_hide_event_from_tui(&event) {
                        return;
                    }
                    with_state_write(&state, |host| {
                        host.record_event(event.topic.clone(), payload_preview(&event.payload));
                    });
                }
            })
            .build();
        if let Err(err) = bot
            .plugin_sdk()
            .sync_disabled_scope_commands(&disabled_commands)
        {
            let warning = trf(
                "startup.command_policy_sync_failed",
                &[("err", err.to_string().as_str())],
            );
            emit_console_log(LogLevel::Warn, "app.host", warning.as_str());
            with_state_write(&state, |host| host.push_warning(warning));
        }
        bot.set_disabled_plugin_ids(disabled_plugins.clone());

        with_state_write(&state, |host| {
            host.set_external_stats(&external_gateway.snapshot());
        });
        spawn_resource_usage_sampler(state.clone());

        let state_for_tick = state.clone();
        let external_gateway_for_tick = external_gateway.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            loop {
                ticker.tick().await;
                let snapshot = external_gateway_for_tick.sweep_timeouts(EXTERNAL_API_TIMEOUT);
                with_state_write(&state_for_tick, |host| host.set_external_stats(&snapshot));
            }
        });

        let state_before = state.clone();
        bot.on_before_start_sync(
            "embedded-before-start",
            Default::default(),
            move |_context| {
                with_state_write(&state_before, |host| {
                    host.set_status("starting");
                    host.push_note(tr("startup.runtime_preparing"));
                });
                Ok(())
            },
        );

        let state_after = state.clone();
        bot.on_after_start_sync(
            "embedded-after-start",
            Default::default(),
            move |_context| {
                with_state_write(&state_after, |host| {
                    host.set_status("running");
                    host.push_note(tr("startup.runtime_started"));
                });
                Ok(())
            },
        );

        let state_before_shutdown = state.clone();
        bot.on_before_process_shutdown_sync(
            "embedded-before-shutdown",
            Default::default(),
            move |_context, process_name| {
                with_state_write(&state_before_shutdown, |host| {
                    host.push_note(trf(
                        "startup.shutting_down_process",
                        &[("process", process_name.as_ref())],
                    ));
                });
                Ok(())
            },
        );

        install_external_event_handlers(
            &bot,
            external_gateway,
            AppHostExternalCommandObserver {
                state: state.clone(),
            },
            help_whitelist,
            llm_runtime,
            bot.plugin_sdk().clone(),
            superuser_manager,
        );

        bot.start()
            .await
            .map_err(|err| format!("failed to start embedded app host: {err}"))?;
        if adapter_autostart {
            attempt_embedded_adapter_autostart(&bot, &state).await;
        }
        with_state_write(&state, |host| {
            host.set_status("running");
            host.push_note("embedded runtime ready");
        });

        let bot = Arc::new(AsyncMutex::new(bot));
        spawn_plugin_cron_scheduler(bot.clone(), state.clone());

        Ok(Self { bot, state })
    }

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
        emit_console_log(
            LogLevel::Info,
            "app.host.reload",
            format!(
                "applying plugin policy from web host (disabled={})",
                disabled_plugin_ids.len()
            ),
        );
        let bot = self.bot.lock().await;
        bot.reload_plugins(disabled_plugin_ids.clone())
            .await
            .map_err(|err| {
                let message = format!("failed to apply plugin reload: {err}");
                emit_console_log(
                    LogLevel::Error,
                    "app.host.reload",
                    format!("plugin policy apply failed: {message}"),
                );
                message
            })?;
        with_state_write(&self.state, |host| {
            host.snapshot.disabled_plugins = disabled_plugin_ids.clone();
            host.push_note(format!(
                "web host applied plugin policy (disabled={})",
                disabled_plugin_ids.len()
            ));
        });
        emit_console_log(
            LogLevel::Info,
            "app.host.reload",
            format!(
                "applied plugin policy from web host (disabled={})",
                disabled_plugin_ids.len()
            ),
        );
        Ok(())
    }

    pub async fn apply_adapter_configs(
        &self,
        adapter_configs: Vec<AdapterConfig>,
    ) -> Result<(), String> {
        let autostart = !adapter_configs.is_empty();
        emit_console_log(
            LogLevel::Info,
            "app.host.reload",
            format!(
                "applying adapter config from web host (count={}, autostart={})",
                adapter_configs.len(),
                autostart
            ),
        );
        let bot = self.bot.lock().await;
        bot.reload_adapters(adapter_configs.clone(), autostart)
            .await
            .map_err(|err| {
                let message = format!("failed to apply adapter reload: {err}");
                emit_console_log(
                    LogLevel::Error,
                    "app.host.reload",
                    format!("adapter config apply failed: {message}"),
                );
                message
            })?;
        with_state_write(&self.state, |host| {
            host.snapshot.adapter_count = adapter_configs.len();
            host.snapshot.adapter_autostart = autostart;
            host.push_note(format!(
                "web host applied adapter config (count={}, autostart={})",
                adapter_configs.len(),
                autostart
            ));
        });
        emit_console_log(
            LogLevel::Info,
            "app.host.reload",
            format!(
                "applied adapter config from web host (count={}, autostart={})",
                adapter_configs.len(),
                autostart
            ),
        );
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        with_state_write(&self.state, |host| {
            host.set_status("stopping");
            host.push_note("shutdown requested");
        });

        let mut bot = self.bot.lock().await;
        match bot.shutdown().await {
            Ok(()) => {
                with_state_write(&self.state, |host| {
                    host.set_status("stopped");
                    host.push_note("embedded runtime stopped");
                });
                Ok(())
            }
            Err(err) => Err(format!("failed to stop embedded app host: {err}")),
        }
    }
}

fn with_state_write(state: &Arc<RwLock<AppHostState>>, updater: impl FnOnce(&mut AppHostState)) {
    if let Ok(mut lock) = state.write() {
        updater(&mut lock);
    }
}

fn spawn_resource_usage_sampler(state: Arc<RwLock<AppHostState>>) {
    let mut sampler = match ResourceUsageSampler::try_new() {
        Ok(sampler) => sampler,
        Err(err) => {
            let warning = format!("resource usage sampler unavailable: {err}");
            emit_console_log(LogLevel::Warn, "app.host", warning.as_str());
            with_state_write(&state, |host| host.push_warning(warning));
            return;
        }
    };

    with_state_write(&state, |host| host.set_resource_usage(sampler.sample()));

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(RESOURCE_USAGE_SAMPLE_INTERVAL);
        loop {
            ticker.tick().await;
            with_state_write(&state, |host| host.set_resource_usage(sampler.sample()));
        }
    });
}

fn spawn_plugin_cron_scheduler(
    bot: Arc<AsyncMutex<LiteyukiBot>>,
    state: Arc<RwLock<AppHostState>>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PLUGIN_CRON_SAMPLE_INTERVAL);
        loop {
            ticker.tick().await;
            let should_stop = state
                .read()
                .ok()
                .is_some_and(|host| host.snapshot.status == "stopped");
            if should_stop {
                break;
            }

            let result = {
                let bot = bot.lock().await;
                bot.plugin_sdk()
                    .run_due_plugin_jobs(bot.disabled_plugin_ids().as_slice(), Some(Utc::now()))
            };
            match result {
                Ok(executed) if executed > 0 => {
                    with_state_write(&state, |host| {
                        host.push_note(format!("plugin cron scheduler executed {executed} job(s)"));
                    });
                }
                Ok(_) => {}
                Err(err) => {
                    let warning = format!("plugin cron scheduler tick failed: {err}");
                    emit_console_log(LogLevel::Warn, "app.host.cron", warning.as_str());
                    with_state_write(&state, |host| host.push_warning(warning));
                }
            }
        }
    });
}

async fn attempt_embedded_adapter_autostart(bot: &LiteyukiBot, state: &Arc<RwLock<AppHostState>>) {
    if let Err(err) = bot.start_adapters().await {
        let warning =
            format!("embedded adapter autostart failed; continuing without adapters: {err}");
        emit_console_log(LogLevel::Warn, "app.host", warning.as_str());
        with_state_write(state, |host| {
            host.push_warning(warning.clone());
            host.push_note("embedded adapter autostart degraded");
        });

        if let Err(stop_err) = bot.stop_adapters().await {
            let stop_warning =
                format!("embedded adapter cleanup after autostart failure reported: {stop_err}");
            emit_console_log(LogLevel::Warn, "app.host", stop_warning.as_str());
            with_state_write(state, |host| {
                host.push_warning(stop_warning.clone());
                host.push_note("embedded adapter cleanup reported an error");
            });
        }
    } else {
        with_state_write(state, |host| {
            host.push_note("embedded adapters ready");
        });
    }
}

struct ResourceUsageSampler {
    pid: sysinfo::Pid,
    system: System,
}

impl ResourceUsageSampler {
    fn try_new() -> Result<Self, String> {
        let pid = get_current_pid().map_err(|err| err.to_string())?;
        let mut system = System::new();
        system.refresh_memory();
        system.refresh_cpu_usage();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::new().with_cpu().with_memory(),
        );
        Ok(Self { pid, system })
    }

    fn sample(&mut self) -> AppHostResourceUsage {
        self.system.refresh_memory();
        self.system.refresh_cpu_usage();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::new().with_cpu().with_memory(),
        );

        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();
        let (process_cpu, process_memory) = self
            .system
            .process(self.pid)
            .map(|process| (process.cpu_usage(), process.memory()))
            .unwrap_or((0.0, 0));

        AppHostResourceUsage {
            cpu: AppHostCpuUsage {
                system_percent: normalize_percent(self.system.global_cpu_usage()),
                process_percent: normalize_percent(process_cpu),
            },
            memory: AppHostMemoryUsage {
                total_bytes: total_memory,
                used_bytes: used_memory,
                process_bytes: process_memory,
                system_percent: usage_percent(used_memory, total_memory),
                process_percent: usage_percent(process_memory, total_memory),
            },
        }
    }
}

fn usage_percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    normalize_percent(((used as f64 / total as f64) * 100.0) as f32)
}

fn normalize_percent(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 100.0)
}

fn update_external_stats(state: &Arc<RwLock<AppHostState>>, snapshot: &ExternalGatewaySnapshot) {
    with_state_write(state, |host| host.set_external_stats(snapshot));
}

#[derive(Clone)]
struct AppHostExternalCommandObserver {
    state: Arc<RwLock<AppHostState>>,
}

impl ExternalCommandObserver for AppHostExternalCommandObserver {
    fn record_stats(&self, snapshot: &ExternalGatewaySnapshot) {
        update_external_stats(&self.state, snapshot);
    }

    fn on_help_whitelist_evaluated(
        &self,
        _event: &liteyukibot_core::SessionEvent,
        matched_entry: Option<&str>,
        whitelist_size: usize,
        _allowed: bool,
    ) {
        if let Some(entry) = matched_entry {
            with_state_write(&self.state, |host| {
                host.push_note(format!(
                    "help whitelist matched entry={entry} size={whitelist_size}"
                ));
            });
        }
    }
}

fn runtime_target_name(target: RuntimeTarget) -> &'static str {
    match target {
        RuntimeTarget::Cli => "cli",
        RuntimeTarget::Web => "web",
        RuntimeTarget::Tauri2 => "tauri2",
        RuntimeTarget::Docker => "docker",
        RuntimeTarget::CliWeb => "cli-web",
        RuntimeTarget::DockerWeb => "docker-web",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::LlmConfigSection;
    use crate::runtime_support::merge_llm_config_sections;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => unsafe {
                    std::env::set_var(self.key, value);
                },
                None => unsafe {
                    std::env::remove_var(self.key);
                },
            }
        }
    }

    fn temp_path(name: &str, ext: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("rsliteyukibot-{name}-{unique}.{ext}"))
    }

    #[test]
    fn plugin_dir_candidates_cover_runtime_and_dev_layouts() {
        let mut dirs = Vec::new();
        let mut seen = HashSet::new();
        let root = PathBuf::from("C:/liteyuki");
        push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, root.as_path(), true);

        assert!(dirs.contains(&root.join("builtin_plugin")));
        assert!(dirs.contains(&root.join("resources").join("builtin_plugin")));
        assert!(dirs.contains(&root.join("src").join("builtin_plugin")));
    }

    #[test]
    fn explicit_plugin_paths_support_directories_and_install_roots() {
        let mut dirs = Vec::new();
        let mut seen = HashSet::new();
        let root = PathBuf::from("C:/liteyuki");
        push_explicit_plugin_dir_candidates(&mut dirs, &mut seen, root.as_path());

        assert!(dirs.contains(&root));
        assert!(dirs.contains(&root.join("builtin_plugin")));
        assert!(dirs.contains(&root.join("resources").join("builtin_plugin")));
        assert!(dirs.contains(&root.join("src").join("builtin_plugin")));
    }

    #[test]
    fn merge_llm_config_sections_prefers_overlay_values() {
        let merged = merge_llm_config_sections(
            Some(LlmConfigSection {
                stream: Some(false),
                provider: Some("openai".to_string()),
                model: Some("gpt-4.1-mini".to_string()),
                temperature: Some(0.6),
                top_p: Some(0.9),
                top_k: Some(16),
                parallel_tool_calls: Some(true),
                command_prefix: Some("/ask".to_string()),
                ..Default::default()
            }),
            LlmConfigSection {
                stream: Some(true),
                model: Some("gpt-4.1".to_string()),
                temperature: Some(0.2),
                top_p: Some(0.8),
                top_k: Some(32),
                parallel_tool_calls: Some(false),
                command_prefix: Some("/qa".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(merged.stream, Some(true));
        assert_eq!(merged.provider.as_deref(), Some("openai"));
        assert_eq!(merged.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(merged.temperature, Some(0.2));
        assert_eq!(merged.top_p, Some(0.8));
        assert_eq!(merged.top_k, Some(32));
        assert_eq!(merged.parallel_tool_calls, Some(false));
        assert_eq!(merged.command_prefix.as_deref(), Some("/qa"));
    }

    #[test]
    fn dedup_warnings_preserves_first_occurrence() {
        let warnings = dedup_warnings(vec![
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
            "c".to_string(),
        ]);

        assert_eq!(warnings, vec!["a", "b", "c"]);
    }

    #[test]
    fn usage_percent_handles_zero_total_and_clamps() {
        assert_eq!(usage_percent(10, 0), 0.0);
        assert_eq!(usage_percent(25, 100), 25.0);
        assert_eq!(usage_percent(150, 100), 100.0);
    }

    #[test]
    fn normalize_percent_handles_invalid_values() {
        assert_eq!(normalize_percent(f32::NAN), 0.0);
        assert_eq!(normalize_percent(-4.0), 0.0);
        assert_eq!(normalize_percent(18.5), 18.5);
        assert_eq!(normalize_percent(180.0), 100.0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn embedded_host_tolerates_adapter_autostart_failures() {
        let _lock = env_lock().lock().expect("env lock should not be poisoned");
        let config_path = temp_path("embedded-host-config", "yaml");
        let llm_config_path = temp_path("embedded-host-llm", "yaml");
        let password_path = temp_path("embedded-host-password", "yaml");
        let config_source = r#"
adapters:
  - id: sse-broken
    enabled: true
    transport: sse
    endpoint:
      url: http://127.0.0.1:1/sse
      timeout_ms: 100
    route:
      inbound_topic: adapter.inbound
      outbound_topic: adapter.outbound
    queue_capacity: 4
"#;
        fs::write(&config_path, config_source).expect("test config should be written");
        let _config_guard = EnvVarGuard::set("LY_CONFIG_PATH", config_path.as_path());
        let _llm_guard = EnvVarGuard::set("LY_LLM_CONFIG_PATH", llm_config_path.as_path());
        let _password_guard = EnvVarGuard::set("LY_PASSWORD_PATH", password_path.as_path());

        let host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
            .await
            .expect("embedded host should keep running when adapters fail");
        let snapshot = host.snapshot();

        assert_eq!(snapshot.status, "running");
        assert_eq!(snapshot.adapter_count, 1);
        assert!(snapshot.adapter_autostart);
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|warning| warning.contains("embedded adapter autostart failed")),
            "expected embedded adapter warning, got {:?}",
            snapshot.warnings
        );
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|warning| warning.contains("adapter sse error")),
            "expected adapter error detail, got {:?}",
            snapshot.warnings
        );

        host.shutdown()
            .await
            .expect("embedded host should shutdown cleanly");

        let _ = fs::remove_file(config_path);
        let _ = fs::remove_file(llm_config_path);
        let _ = fs::remove_file(password_path);
    }
}
