use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid};
use tokio::sync::Mutex as AsyncMutex;

use crate::adapter::AdapterManager;
use crate::app_config::{
    AppConfigDoc, LlmConfigSection, LlmRuntimeConfig, ensure_default_config_files,
    load_adapter_configs, load_app_config_from_path, load_app_config_with_warnings,
    prime_reload_warning_state, resolve_app_locale, resolve_disabled_plugins,
    resolve_disabled_scope_commands, resolve_help_whitelist, resolve_llm_config,
    validate_app_config,
};
use crate::command_registry::{
    AdapterProtocol, BuiltinCommandId, CommandNameOverrides, CommandScope,
    command_argument_for_message, matches_builtin_command_message,
};
use crate::i18n::{reload_catalog as reload_i18n_catalog, set_current_locale, tr, trf};
use crate::llm::{
    LlmClientError, LlmPromptProfile, LlmPromptStore, OpenAiResponsesClient, compose_user_prompt,
};
use crate::onebot_support::{
    build_onebot_v11_text_reply_payload, is_help_command, is_help_session_allowed,
    is_onebot_private_message, is_onebot_v11_payload, matched_help_whitelist_entry,
    parse_su_password_argument, payload_preview, should_hide_event_from_tui, value_to_string,
};
use crate::superuser::SuperuserManager;
use crate::{
    AdapterPacket, BotRuntimeConfig, LiteyukiBot, LogLevel, LogMode, PluginSdk, Rule,
    RuntimeSettings, RuntimeTarget, TimeZone, TimestampFormat, emit_console_log,
};

const APP_TITLE: &str = "RsLiteyukiBot";
const EXTERNAL_API_TIMEOUT: Duration = Duration::from_secs(12);
const RESOURCE_USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const LLM_CONFIG_PATHS: [&str; 2] = ["llm-config.yaml", "llm-config.toml"];
const LLM_PROMPT_STORE_PATH: &str = "llm-prompts.json";
const PASSWORD_CONFIG_PATH: &str = "password.yaml";
const BUILTIN_PLUGIN_DIRS: [&str; 2] = ["builtin_plugin", "resources/builtin_plugin"];
const DEV_BUILTIN_PLUGIN_DIRS: [&str; 1] = ["src/builtin_plugin"];
const MAX_STATUS_NOTES: usize = 32;

static LLM_API_KEY_ROUND_ROBIN: AtomicU64 = AtomicU64::new(0);

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
            runtime_target: runtime_target_name(RuntimeTarget::Tauri2).to_string(),
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

#[derive(Debug)]
struct AppHostState {
    snapshot: AppHostSnapshot,
}

impl AppHostState {
    fn new(snapshot: AppHostSnapshot) -> Self {
        Self { snapshot }
    }

    fn snapshot(&self) -> AppHostSnapshot {
        self.snapshot.clone()
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
    pub async fn start_tauri() -> Result<Self, String> {
        Self::start_for_target(RuntimeTarget::Tauri2).await
    }

    pub async fn start_for_target(target: RuntimeTarget) -> Result<Self, String> {
        if let Err(err) = ensure_default_config_files() {
            emit_console_log(
                LogLevel::Warn,
                "app.host",
                trf(
                    "startup.ensure_default_config_failed",
                    &[("err", err.to_string().as_str())],
                ),
            );
        }
        if let Err(err) = ensure_default_llm_config_file() {
            emit_console_log(
                LogLevel::Warn,
                "app.host",
                trf(
                    "startup.ensure_default_llm_config_failed",
                    &[("err", err.as_str())],
                ),
            );
        }

        let settings = match RuntimeSettings::try_load() {
            Ok(settings) => settings,
            Err(err) => {
                emit_console_log(
                    LogLevel::Warn,
                    "app.host",
                    trf(
                        "startup.runtime_settings_fallback",
                        &[("err", err.to_string().as_str())],
                    ),
                );
                RuntimeSettings::default()
            }
        };
        let _ = settings.clone().install_global();
        let mut runtime_config = RuntimeSettings::global_runtime_config().clone();
        runtime_config.logger.min_level = LogLevel::Error;

        let (app_config, mut warnings) = load_app_config_with_llm_overlay();
        prime_reload_warning_state(&app_config);
        apply_runtime_log_overrides_from_app_config(&mut runtime_config, &app_config);
        let effective_runtime_config = target.tune_runtime_config(runtime_config.clone());
        let adapter_configs = load_adapter_configs(&app_config)
            .map_err(|err| format!("failed to load adapters: {err}"))?;
        let adapter_autostart = !adapter_configs.is_empty();
        let help_whitelist = Arc::new(RwLock::new(resolve_help_whitelist(&app_config)));
        let locale = resolve_app_locale(&app_config);
        let llm_config = resolve_llm_config(&app_config);
        let disabled_commands = resolve_disabled_scope_commands(&app_config);
        let disabled_plugins = resolve_disabled_plugins(&app_config);
        let llm_runtime = LlmCommandRuntime::new(llm_config.command_prefix.clone());
        let external_gateway = ExternalGateway::new();
        let plugin_dirs = resolve_builtin_plugin_dirs();
        set_current_locale(locale);
        warnings.extend(reload_i18n_catalog(plugin_dirs.iter()));
        warnings = dedup_warnings(warnings);

        let superuser_manager =
            match SuperuserManager::load_or_init(resolve_password_config_path().as_path()) {
                Ok(manager) => manager,
                Err(err) => {
                    emit_console_log(
                        LogLevel::Warn,
                        "app.host",
                        trf(
                            "startup.password_config_fallback",
                            &[("err", err.to_string().as_str())],
                        ),
                    );
                    SuperuserManager::in_memory()
                }
            };

        let state = Arc::new(RwLock::new(AppHostState::new(AppHostSnapshot {
            app_name: APP_TITLE.to_string(),
            status: "starting".to_string(),
            runtime_target: runtime_target_name(target).to_string(),
            locale: locale.as_str().to_string(),
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
        bot.on_before_start_sync("tauri-before-start", Default::default(), move |_context| {
            with_state_write(&state_before, |host| {
                host.set_status("starting");
                host.push_note(tr("startup.runtime_preparing"));
            });
            Ok(())
        });

        let state_after = state.clone();
        bot.on_after_start_sync("tauri-after-start", Default::default(), move |_context| {
            with_state_write(&state_after, |host| {
                host.set_status("running");
                host.push_note(tr("startup.runtime_started"));
            });
            Ok(())
        });

        let state_before_shutdown = state.clone();
        bot.on_before_process_shutdown_sync(
            "tauri-before-shutdown",
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
            state.clone(),
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

        Ok(Self {
            bot: Arc::new(AsyncMutex::new(bot)),
            state,
        })
    }

    pub fn snapshot(&self) -> AppHostSnapshot {
        self.state
            .read()
            .expect("embedded app host state lock should not be poisoned")
            .snapshot()
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

#[derive(Clone)]
struct LlmCommandRuntime {
    command_prefix: Arc<RwLock<String>>,
}

impl LlmCommandRuntime {
    fn new(command_prefix: impl Into<String>) -> Self {
        Self {
            command_prefix: Arc::new(RwLock::new(command_prefix.into())),
        }
    }

    fn command_prefix(&self) -> String {
        self.command_prefix
            .read()
            .expect("llm command prefix lock should not be poisoned")
            .clone()
    }
}

#[derive(Debug, Clone, Default)]
struct ExternalGatewaySnapshot {
    command_hits: u64,
    api_requests: u64,
    api_success: u64,
    api_failed: u64,
    api_timeouts: u64,
    api_inflight: usize,
}

#[derive(Debug)]
struct PendingApiCall {
    started_at: Instant,
}

#[derive(Debug, Default)]
struct ExternalGatewayState {
    command_hits: u64,
    api_requests: u64,
    api_success: u64,
    api_failed: u64,
    api_timeouts: u64,
    pending: HashMap<String, PendingApiCall>,
}

impl ExternalGatewayState {
    fn snapshot(&self) -> ExternalGatewaySnapshot {
        ExternalGatewaySnapshot {
            command_hits: self.command_hits,
            api_requests: self.api_requests,
            api_success: self.api_success,
            api_failed: self.api_failed,
            api_timeouts: self.api_timeouts,
            api_inflight: self.pending.len(),
        }
    }
}

#[derive(Clone, Default)]
struct ExternalGateway {
    state: Arc<Mutex<ExternalGatewayState>>,
    echo_seq: Arc<AtomicU64>,
}

impl ExternalGateway {
    fn new() -> Self {
        Self::default()
    }

    fn snapshot(&self) -> ExternalGatewaySnapshot {
        self.state
            .lock()
            .expect("external gateway lock should not be poisoned")
            .snapshot()
    }

    fn next_echo(&self, prefix: &str) -> String {
        let seq = self.echo_seq.fetch_add(1, Ordering::SeqCst);
        format!("{prefix}-{seq}")
    }

    fn record_command_hit(&self) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        state.command_hits = state.command_hits.saturating_add(1);
        state.snapshot()
    }

    fn track_request(&self, echo: String) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        state.api_requests = state.api_requests.saturating_add(1);
        state.pending.insert(
            echo,
            PendingApiCall {
                started_at: Instant::now(),
            },
        );
        state.snapshot()
    }

    fn mark_send_failed(&self, echo: &str) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        if state.pending.remove(echo).is_some() {
            state.api_failed = state.api_failed.saturating_add(1);
        }
        state.snapshot()
    }

    fn observe_payload(&self, payload: &Value, timeout: Duration) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        sweep_pending_timeouts(&mut state, timeout);

        if let Some((echo, success)) = parse_onebot_v11_api_response(payload)
            && state.pending.remove(&echo).is_some()
        {
            if success {
                state.api_success = state.api_success.saturating_add(1);
            } else {
                state.api_failed = state.api_failed.saturating_add(1);
            }
        }

        state.snapshot()
    }

    fn sweep_timeouts(&self, timeout: Duration) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        sweep_pending_timeouts(&mut state, timeout);
        state.snapshot()
    }
}

fn sweep_pending_timeouts(state: &mut ExternalGatewayState, timeout: Duration) {
    let expired: Vec<String> = state
        .pending
        .iter()
        .filter_map(|(echo, call)| {
            if call.started_at.elapsed() >= timeout {
                Some(echo.clone())
            } else {
                None
            }
        })
        .collect();

    if expired.is_empty() {
        return;
    }

    for echo in expired {
        if state.pending.remove(&echo).is_some() {
            state.api_timeouts = state.api_timeouts.saturating_add(1);
        }
    }
}

fn parse_onebot_v11_api_response(payload: &Value) -> Option<(String, bool)> {
    let object = payload.as_object()?;
    if !object.contains_key("status") && !object.contains_key("retcode") {
        return None;
    }

    let echo = object.get("echo").and_then(value_to_string)?;
    let success = object
        .get("status")
        .and_then(Value::as_str)
        .map(|status| status.eq_ignore_ascii_case("ok"))
        .or_else(|| {
            object
                .get("retcode")
                .and_then(Value::as_i64)
                .map(|code| code == 0)
        })
        .unwrap_or(false);
    Some((echo, success))
}

fn matches_external_ask_command(message: &str, llm_runtime: &LlmCommandRuntime) -> bool {
    let command_prefix = llm_runtime.command_prefix();
    matches_builtin_command_message(
        BuiltinCommandId::Ask,
        message,
        CommandScope::Adapter(AdapterProtocol::OneBot11),
        CommandNameOverrides {
            onebot_ask_prefix: Some(command_prefix.as_str()),
        },
    )
}

fn llm_usage_text(command_prefix: &str) -> String {
    trf("main.ask.usage", &[("command", command_prefix)])
}

fn command_disabled_text(command_name: &str) -> String {
    trf("main.command.disabled", &[("command", command_name)])
}

fn install_external_event_handlers(
    bot: &LiteyukiBot,
    gateway: ExternalGateway,
    state: Arc<RwLock<AppHostState>>,
    help_whitelist: Arc<RwLock<HashSet<String>>>,
    llm_runtime: LlmCommandRuntime,
    plugin_sdk: PluginSdk,
    superuser_manager: SuperuserManager,
) {
    let adapter_manager = bot.adapter_manager().clone();
    let gateway_for_su = gateway.clone();
    let state_for_su = state.clone();
    let superuser_for_su = superuser_manager.clone();
    let plugin_sdk_for_su = plugin_sdk.clone();
    bot.on_message(
        "builtin.external.su",
        Rule::new("command.su", |event| async move {
            parse_su_password_argument(event.message.as_ref()).is_some()
        }),
        510,
        true,
        move |event| {
            let adapter_manager = adapter_manager.clone();
            let gateway = gateway_for_su.clone();
            let state = state_for_su.clone();
            let superuser_manager = superuser_for_su.clone();
            let plugin_sdk = plugin_sdk_for_su.clone();
            async move {
                handle_external_su_command(
                    &adapter_manager,
                    &gateway,
                    &state,
                    &plugin_sdk,
                    &superuser_manager,
                    event,
                )
                .await
            }
        },
    );

    let adapter_manager = bot.adapter_manager().clone();
    let gateway_for_help = gateway.clone();
    let state_for_help = state.clone();
    let help_whitelist_for_help = help_whitelist.clone();
    let superuser_for_help = superuser_manager.clone();
    let llm_runtime_for_help = llm_runtime.clone();
    let plugin_sdk_for_help = plugin_sdk.clone();
    bot.on_message(
        "builtin.external.help",
        Rule::new("command.help", |event| async move {
            is_help_command(event.message.as_ref())
        }),
        500,
        true,
        move |event| {
            let adapter_manager = adapter_manager.clone();
            let gateway = gateway_for_help.clone();
            let state = state_for_help.clone();
            let help_whitelist = help_whitelist_for_help.clone();
            let superuser_manager = superuser_for_help.clone();
            let llm_runtime = llm_runtime_for_help.clone();
            let plugin_sdk = plugin_sdk_for_help.clone();
            async move {
                if plugin_sdk.is_builtin_command_disabled("adapter:onebot11", "/help") {
                    let text = command_disabled_text("/help");
                    return reply_external_text(
                        &adapter_manager,
                        &gateway,
                        &state,
                        event.as_ref(),
                        text.as_str(),
                        "command-disabled-help",
                    )
                    .await;
                }
                if !superuser_manager.is_superuser(event.as_ref()) {
                    return reply_external_text(
                        &adapter_manager,
                        &gateway,
                        &state,
                        event.as_ref(),
                        tr("main.auth.su_required").as_str(),
                        "su-required-help",
                    )
                    .await;
                }

                let (allowed, matched_entry, whitelist_size) = help_whitelist
                    .read()
                    .map(|set| {
                        let matched = matched_help_whitelist_entry(event.as_ref(), &set);
                        let allowed = is_help_session_allowed(event.as_ref(), &set);
                        (allowed, matched, set.len())
                    })
                    .unwrap_or_else(|_| (false, None, 0));
                if let Some(entry) = matched_entry {
                    with_state_write(&state, |host| {
                        host.push_note(format!(
                            "help whitelist matched entry={entry} size={whitelist_size}"
                        ));
                    });
                }
                if !allowed {
                    return Ok(());
                }
                reply_help_command(
                    &adapter_manager,
                    &gateway,
                    &state,
                    &llm_runtime,
                    &plugin_sdk,
                    event,
                )
                .await
            }
        },
    );

    let adapter_manager = bot.adapter_manager().clone();
    let gateway_for_ask = gateway.clone();
    let state_for_ask = state.clone();
    let llm_runtime_for_rule = llm_runtime.clone();
    let superuser_for_ask = superuser_manager.clone();
    let plugin_sdk_for_ask = plugin_sdk.clone();
    bot.on_message(
        "builtin.external.ask",
        Rule::new("command.ask", move |event| {
            let llm_runtime = llm_runtime_for_rule.clone();
            async move { matches_external_ask_command(event.message.as_ref(), &llm_runtime) }
        }),
        490,
        true,
        move |event| {
            let adapter_manager = adapter_manager.clone();
            let gateway = gateway_for_ask.clone();
            let state = state_for_ask.clone();
            let llm_runtime = llm_runtime.clone();
            let superuser_manager = superuser_for_ask.clone();
            let plugin_sdk = plugin_sdk_for_ask.clone();
            async move {
                let ask_command = llm_runtime.command_prefix();
                if plugin_sdk.is_builtin_command_disabled("adapter:onebot11", ask_command.as_str())
                {
                    let text = command_disabled_text(ask_command.as_str());
                    return reply_external_text(
                        &adapter_manager,
                        &gateway,
                        &state,
                        event.as_ref(),
                        text.as_str(),
                        "command-disabled-ask",
                    )
                    .await;
                }
                if !superuser_manager.is_superuser(event.as_ref()) {
                    return reply_external_text(
                        &adapter_manager,
                        &gateway,
                        &state,
                        event.as_ref(),
                        tr("main.auth.su_required").as_str(),
                        "su-required-ask",
                    )
                    .await;
                }
                reply_ask_command(&adapter_manager, &gateway, &state, &llm_runtime, event).await
            }
        },
    );
}

async fn handle_external_su_command(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    state: &Arc<RwLock<AppHostState>>,
    plugin_sdk: &PluginSdk,
    superuser_manager: &SuperuserManager,
    event: Arc<crate::SessionEvent>,
) -> Result<(), String> {
    let Some(password_raw) = parse_su_password_argument(event.message.as_ref()) else {
        return Ok(());
    };
    if plugin_sdk.is_builtin_command_disabled("adapter:onebot11", "/su") {
        let text = command_disabled_text("/su");
        return reply_external_text(
            adapter_manager,
            gateway,
            state,
            event.as_ref(),
            text.as_str(),
            "command-disabled-su",
        )
        .await;
    }

    if is_onebot_v11_payload(&event.payload) && !is_onebot_private_message(event.as_ref()) {
        return reply_external_text(
            adapter_manager,
            gateway,
            state,
            event.as_ref(),
            tr("main.su.private_only").as_str(),
            "su-private-only",
        )
        .await;
    }

    if password_raw.trim().is_empty() {
        return reply_external_text(
            adapter_manager,
            gateway,
            state,
            event.as_ref(),
            tr("main.su.usage").as_str(),
            "su-usage",
        )
        .await;
    }

    if !superuser_manager.verify_password(password_raw.as_str()) {
        return reply_external_text(
            adapter_manager,
            gateway,
            state,
            event.as_ref(),
            tr("main.su.denied").as_str(),
            "su-denied",
        )
        .await;
    }

    let promoted = superuser_manager
        .promote_user(event.as_ref())
        .map_err(|err| {
            let err = err.to_string();
            trf("main.su.persist_failed", &[("err", err.as_str())])
        })?;
    let text = if promoted.added {
        tr("main.su.enabled.added")
    } else {
        tr("main.su.enabled")
    };
    reply_external_text(
        adapter_manager,
        gateway,
        state,
        event.as_ref(),
        text.as_str(),
        "su-granted",
    )
    .await
}

async fn reply_help_command(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    state: &Arc<RwLock<AppHostState>>,
    llm_runtime: &LlmCommandRuntime,
    plugin_sdk: &PluginSdk,
    event: Arc<crate::SessionEvent>,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    let help_text = crate::onebot_support::render_external_help_text_with_plugins(
        llm_runtime.command_prefix().as_str(),
        Some(plugin_sdk),
    );
    reply_external_text(
        adapter_manager,
        gateway,
        state,
        event.as_ref(),
        help_text.as_str(),
        "liteyuki-help",
    )
    .await
}

async fn reply_ask_command(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    state: &Arc<RwLock<AppHostState>>,
    llm_runtime: &LlmCommandRuntime,
    event: Arc<crate::SessionEvent>,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    update_external_stats(state, &gateway.record_command_hit());

    let command_prefix = llm_runtime.command_prefix();
    let prompt = command_argument_for_message(
        BuiltinCommandId::Ask,
        event.message.as_ref(),
        CommandScope::Adapter(AdapterProtocol::OneBot11),
        CommandNameOverrides {
            onebot_ask_prefix: Some(command_prefix.as_str()),
        },
    )
    .unwrap_or_default();
    let reply_text = if prompt.is_empty() {
        llm_usage_text(command_prefix.as_str())
    } else {
        match generate_llm_reply(&prompt).await {
            Ok(output) => {
                if output.trim().is_empty() {
                    tr("main.llm.empty")
                } else {
                    output
                }
            }
            Err(err) => trf("main.llm.failed", &[("err", err.as_str())]),
        }
    };

    let echo = gateway.next_echo("liteyuki-ask");
    let payload = build_onebot_v11_text_reply_payload(event.as_ref(), &echo, &reply_text)
        .ok_or_else(|| "failed to build onebot v11 ask response".to_string())?;
    dispatch_onebot_reply(
        adapter_manager,
        gateway,
        state,
        event.as_ref(),
        format!("ask-{}", event.event_id),
        echo,
        payload,
    )
    .await
}

async fn reply_external_text(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    state: &Arc<RwLock<AppHostState>>,
    event: &crate::SessionEvent,
    text: &str,
    echo_prefix: &str,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    update_external_stats(state, &gateway.record_command_hit());
    let echo = gateway.next_echo(echo_prefix);
    let payload = build_onebot_v11_text_reply_payload(event, &echo, text)
        .ok_or_else(|| "failed to build onebot v11 text response".to_string())?;
    dispatch_onebot_reply(
        adapter_manager,
        gateway,
        state,
        event,
        format!("{echo_prefix}-{}", event.event_id),
        echo,
        payload,
    )
    .await
}

async fn dispatch_onebot_reply(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    state: &Arc<RwLock<AppHostState>>,
    event: &crate::SessionEvent,
    packet_id: String,
    echo: String,
    payload: Value,
) -> Result<(), String> {
    let adapter_id = event
        .payload
        .get("_adapter_id")
        .and_then(value_to_string)
        .ok_or_else(|| "missing adapter id in inbound payload".to_string())?;

    update_external_stats(state, &gateway.track_request(echo.clone()));
    let packet = AdapterPacket::new(packet_id, "onebot.v11.api.send_msg", payload);
    let send_result = adapter_manager.send(&adapter_id, packet).await;
    if let Err(err) = send_result {
        update_external_stats(state, &gateway.mark_send_failed(&echo));
        return Err(format!("send reply failed: {err}"));
    }
    Ok(())
}

fn update_external_stats(state: &Arc<RwLock<AppHostState>>, snapshot: &ExternalGatewaySnapshot) {
    with_state_write(state, |host| host.set_external_stats(snapshot));
}

async fn generate_llm_reply(prompt: &str) -> Result<String, String> {
    let llm_config = current_llm_runtime_config()?;
    if !llm_config.enabled {
        return Err(tr("main.llm.disabled"));
    }
    if !llm_config.provider.eq_ignore_ascii_case("openai") {
        return Err(trf(
            "main.llm.provider.unsupported",
            &[("provider", llm_config.provider.as_str())],
        ));
    }
    let Some(api_key) = pick_next_api_key(&llm_config) else {
        return Err(tr("main.llm.api_key_missing"));
    };
    let prompt_profile = current_active_prompt_profile()?;
    let composed_prompt = compose_user_prompt(prompt, prompt_profile.soul.as_str());

    let client = OpenAiResponsesClient::from_runtime_with_api_key(&llm_config, &api_key)
        .map_err(|err: LlmClientError| err.to_string())?;
    client
        .generate(composed_prompt.as_str())
        .await
        .map_err(|err| err.to_string())
}

fn pick_next_api_key(llm_config: &LlmRuntimeConfig) -> Option<String> {
    let key_count = llm_config.api_keys.len();
    if key_count == 0 {
        return None;
    }
    let index = LLM_API_KEY_ROUND_ROBIN.fetch_add(1, Ordering::SeqCst) as usize % key_count;
    llm_config.api_keys.get(index).cloned()
}

fn current_llm_runtime_config() -> Result<LlmRuntimeConfig, String> {
    let doc = load_current_app_config_doc()?;
    Ok(resolve_llm_config(&doc))
}

fn load_current_app_config_doc() -> Result<AppConfigDoc, String> {
    let (doc, _) = load_app_config_with_llm_overlay();
    Ok(doc)
}

fn current_active_prompt_profile() -> Result<LlmPromptProfile, String> {
    let store = load_llm_prompt_store()?;
    Ok(store.active_profile())
}

fn load_llm_prompt_store() -> Result<LlmPromptStore, String> {
    let path = resolve_llm_prompt_store_path();
    LlmPromptStore::load_or_default_from_path(path.as_path())
}

fn resolve_llm_prompt_store_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_LLM_PROMPT_STORE_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    PathBuf::from(LLM_PROMPT_STORE_PATH)
}

fn resolve_password_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_PASSWORD_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    PathBuf::from(PASSWORD_CONFIG_PATH)
}

fn resolve_builtin_plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(raw) = std::env::var("LY_PLUGIN_DIRS") {
        for path in std::env::split_paths(&raw) {
            push_explicit_plugin_dir_candidates(&mut dirs, &mut seen, path.as_path());
        }
    }

    if let Ok(current_dir) = std::env::current_dir() {
        push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, current_dir.as_path(), true);
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent()
    {
        push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, parent, false);
    }

    dirs
}

fn push_explicit_plugin_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    path: &std::path::Path,
) {
    push_unique_plugin_path(dirs, seen, path.to_path_buf());
    push_runtime_plugin_dir_candidates(dirs, seen, path, true);
}

fn push_runtime_plugin_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    root: &std::path::Path,
    include_dev_fallback: bool,
) {
    for candidate in BUILTIN_PLUGIN_DIRS {
        push_unique_plugin_path(dirs, seen, root.join(candidate));
    }
    if include_dev_fallback {
        for candidate in DEV_BUILTIN_PLUGIN_DIRS {
            push_unique_plugin_path(dirs, seen, root.join(candidate));
        }
    }
}

fn push_unique_plugin_path(dirs: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        dirs.push(path);
    }
}

fn ensure_default_llm_config_file() -> Result<(), String> {
    if let Ok(path) = std::env::var("LY_LLM_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return ensure_llm_config_file(std::path::Path::new(path.trim()));
    }

    if LLM_CONFIG_PATHS
        .iter()
        .map(PathBuf::from)
        .any(|path| path.exists())
    {
        return Ok(());
    }

    ensure_llm_config_file(std::path::Path::new(LLM_CONFIG_PATHS[0]))
}

fn ensure_llm_config_file(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create llm config parent directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    let template = match ext.as_deref() {
        Some("toml") => {
            "[llm]\nenabled = false\nprovider = \"openai\"\nbase_url = \"https://tokenflux.dev/v1\"\nmodel = \"gpt-4.1-mini\"\ntimeout_seconds = 20\ncommand_prefix = \"/ask\"\napi_keys = []\n"
        }
        _ => {
            "llm:\n  enabled: false\n  provider: openai\n  base_url: https://tokenflux.dev/v1\n  model: gpt-4.1-mini\n  timeout_seconds: 20\n  command_prefix: /ask\n  api_keys: []\n"
        }
    };
    std::fs::write(path, template)
        .map_err(|err| format!("failed to write llm config {}: {err}", path.display()))?;
    Ok(())
}

fn load_app_config_with_llm_overlay() -> (AppConfigDoc, Vec<String>) {
    let (mut app_config, mut warnings) = load_app_config_with_warnings(false);
    if let Some(path) = resolve_llm_config_path() {
        match load_app_config_from_path(path.as_path()) {
            Ok(overlay_doc) => {
                if let Some(overlay_llm) = overlay_doc.llm {
                    app_config.llm = Some(merge_llm_config_sections(
                        app_config.llm.take(),
                        overlay_llm,
                    ));
                }
            }
            Err(err) => {
                let path_display = path.display().to_string();
                let err_text = err.to_string();
                warnings.push(
                    trf(
                        "startup.llm_overlay_load_failed",
                        &[("path", path_display.as_str()), ("err", err_text.as_str())],
                    )
                    .to_string(),
                );
            }
        }
    }
    warnings.extend(validate_app_config(&app_config));
    warnings = dedup_warnings(warnings);
    (app_config, warnings)
}

fn merge_llm_config_sections(
    base: Option<LlmConfigSection>,
    overlay: LlmConfigSection,
) -> LlmConfigSection {
    let mut merged = base.unwrap_or_default();
    if overlay.enabled.is_some() {
        merged.enabled = overlay.enabled;
    }
    if overlay.provider.is_some() {
        merged.provider = overlay.provider;
    }
    if overlay.base_url.is_some() {
        merged.base_url = overlay.base_url;
    }
    if overlay.provider_urls.is_some() {
        merged.provider_urls = overlay.provider_urls;
    }
    if overlay.api_keys.is_some() {
        merged.api_keys = overlay.api_keys;
    }
    if overlay.api_key.is_some() {
        merged.api_key = overlay.api_key;
    }
    if overlay.model.is_some() {
        merged.model = overlay.model;
    }
    if overlay.timeout_seconds.is_some() {
        merged.timeout_seconds = overlay.timeout_seconds;
    }
    if overlay.system_prompt.is_some() {
        merged.system_prompt = overlay.system_prompt;
    }
    if overlay.command_prefix.is_some() {
        merged.command_prefix = overlay.command_prefix;
    }
    merged
}

fn resolve_llm_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("LY_LLM_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return Some(PathBuf::from(path));
    }
    LLM_CONFIG_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn apply_runtime_log_overrides_from_app_config(
    runtime_config: &mut BotRuntimeConfig,
    app_config: &AppConfigDoc,
) {
    let runtime = app_config
        .rust
        .as_ref()
        .and_then(|section| section.runtime.as_ref())
        .or(app_config.runtime.as_ref());
    if let Some(runtime) = runtime {
        if let Some(worker_count) = runtime.worker_count
            && worker_count > 0
        {
            runtime_config.worker_count = worker_count;
        }
        if let Some(ingress_queue) = runtime.ingress_queue
            && ingress_queue > 0
        {
            runtime_config.ingress_queue = ingress_queue;
        }
        if let Some(worker_queue) = runtime.worker_queue
            && worker_queue > 0
        {
            runtime_config.worker_queue = worker_queue;
        }
    }

    let log = app_config
        .rust
        .as_ref()
        .and_then(|section| section.log.as_ref())
        .or(app_config.log.as_ref());
    if let Some(log) = log {
        if let Some(mode) = log.mode.as_deref()
            && let Some(mode) = LogMode::parse(mode)
        {
            runtime_config.logger.mode = mode;
        }
        if let Some(level) = log.level.as_deref()
            && let Some(level) = LogLevel::parse(level)
        {
            runtime_config.logger.min_level = level;
        }
        if let Some(timezone) = log.timezone.as_deref()
            && let Some(timezone) = TimeZone::parse(timezone)
        {
            runtime_config.logger.timezone = timezone;
        }
        if let Some(timestamp_format) = log.timestamp_format.as_deref() {
            if timestamp_format.trim().eq_ignore_ascii_case("custom") {
                let pattern = log
                    .timestamp_pattern
                    .as_deref()
                    .unwrap_or("%Y-%m-%d %H:%M:%S")
                    .to_string();
                runtime_config.logger.timestamp_format = TimestampFormat::Custom(pattern);
            } else {
                runtime_config.logger.timestamp_format = TimestampFormat::parse(timestamp_format);
            }
        } else if let Some(pattern) = log.timestamp_pattern.as_deref() {
            runtime_config.logger.timestamp_format = TimestampFormat::Custom(pattern.to_string());
        }
    }
}

fn describe_runtime_config(runtime_config: &BotRuntimeConfig) -> String {
    format!(
        "workers={}, ingress_queue={}, worker_queue={}, log_mode={}, log_level={}, log_tz={}, log_ts={}",
        runtime_config.worker_count,
        runtime_config.ingress_queue,
        runtime_config.worker_queue,
        runtime_config.logger.mode,
        runtime_config.logger.min_level,
        runtime_config.logger.timezone,
        runtime_config.logger.timestamp_format
    )
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

fn dedup_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for warning in warnings {
        if seen.insert(warning.clone()) {
            output.push(warning);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
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
                provider: Some("openai".to_string()),
                model: Some("gpt-4.1-mini".to_string()),
                command_prefix: Some("/ask".to_string()),
                ..Default::default()
            }),
            LlmConfigSection {
                model: Some("gpt-4.1".to_string()),
                command_prefix: Some("/qa".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(merged.provider.as_deref(), Some("openai"));
        assert_eq!(merged.model.as_deref(), Some("gpt-4.1"));
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
