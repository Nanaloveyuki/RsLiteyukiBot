use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use liteyukibot_core::PluginSdk;
use liteyukibot_core::adapter::AdapterManager;
use liteyukibot_core::session::SessionEvent;
use liteyukibot_core::{
    AdapterPacket, LiteyukiBot, LogLevel, LogMode, Rule, RuntimeSettings, RuntimeTarget, TimeZone,
    TimestampFormat,
};
use serde_json::Value;
use tokio::sync::mpsc;

mod app_config;
mod command_registry;
mod config_edit;
mod llm;
mod onebot_support;
mod superuser;
mod tui;

use crate::llm::{
    LlmClientError, LlmPromptPreview, LlmPromptProfile, LlmPromptStore, OpenAiResponsesClient,
    build_prompt_preview, compose_user_prompt,
};
use app_config::*;
use command_registry::{
    AdapterProtocol, BuiltinCommandId, CommandNameOverrides, CommandScope,
    command_argument_for_message, matches_builtin_command_message,
};
use onebot_support::*;
use superuser::SuperuserManager;

const APP_TITLE: &str = "RsLiteyukiBot";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;
const EXTERNAL_API_TIMEOUT: Duration = Duration::from_secs(12);
const LLM_CONFIG_PATHS: [&str; 2] = ["llm-config.yaml", "llm-config.toml"];
const LLM_PROMPT_STORE_PATH: &str = "llm-prompts.json";
const PASSWORD_CONFIG_PATH: &str = "password.yaml";
const DEFAULT_LLM_PROVIDER_BASE_URL: &str = "https://api.openai.com";
const BUILTIN_PLUGIN_DIR: &str = "src/builtin_plugin";

static LLM_API_KEY_ROUND_ROBIN: AtomicU64 = AtomicU64::new(0);

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

    fn shared_command_prefix(&self) -> Arc<RwLock<String>> {
        self.command_prefix.clone()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn set_command_prefix(&self, command_prefix: impl Into<String>) {
        *self
            .command_prefix
            .write()
            .expect("llm command prefix lock should not be poisoned") = command_prefix.into();
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

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(err) = ensure_default_config_files() {
        eprintln!("failed to ensure default config files: {err}");
    }
    if let Err(err) = ensure_default_llm_config_file() {
        eprintln!("failed to ensure default llm config file: {err}");
    }

    let settings = match RuntimeSettings::try_load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("failed to load runtime config from file/env, fallback to default: {err}");
            RuntimeSettings::default()
        }
    };
    let _ = settings.clone().install_global();
    let mut runtime_config = RuntimeSettings::global_runtime_config().clone();
    runtime_config.logger.min_level = LogLevel::Error;

    let (app_config, app_config_warnings) = load_app_config_with_llm_overlay();
    for warning in app_config_warnings {
        eprintln!("{warning}");
    }
    prime_reload_warning_state(&app_config);
    let target = resolve_runtime_target();
    apply_runtime_log_overrides_from_app_config(&mut runtime_config, &app_config);
    let effective_runtime_config = target.tune_runtime_config(runtime_config.clone());
    let adapter_configs = load_adapter_configs(&app_config)?;
    let adapter_autostart = !adapter_configs.is_empty();
    let help_whitelist = Arc::new(RwLock::new(resolve_help_whitelist(&app_config)));
    let tui_config = resolve_tui_config(&app_config);
    let llm_config = resolve_llm_config(&app_config);
    let disabled_commands = resolve_disabled_scope_commands(&app_config);
    let disabled_plugins = resolve_disabled_plugins(&app_config);
    let llm_runtime = LlmCommandRuntime::new(llm_config.command_prefix.clone());
    let external_gateway = ExternalGateway::new();
    let superuser_manager =
        match SuperuserManager::load_or_init(resolve_password_config_path().as_path()) {
            Ok(manager) => manager,
            Err(err) => {
                eprintln!(
                    "failed to load password config, fallback to in-memory superuser manager: {err}"
                );
                SuperuserManager::in_memory()
            }
        };

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<tui::UiEvent>();
    let ui_tx_for_handler = ui_tx.clone();
    let whitelist_size = help_whitelist
        .read()
        .map(|set| set.len())
        .unwrap_or_default();
    if whitelist_size > 0 {
        let _ = ui_tx.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: format!(
                "external /help whitelist enabled (sessions={})",
                whitelist_size
            ),
        });
    }
    let _ = ui_tx.send(tui::UiEvent::Log {
        level: tui::UiLevel::Info,
        message: format!("LLM command prefix: {}", llm_runtime.command_prefix()),
    });
    if superuser_manager.using_dynamic_password() {
        let _ = ui_tx.send(tui::UiEvent::Log {
            level: tui::UiLevel::Warn,
            message: format!(
                "SU dynamic password (this startup only): {}",
                superuser_manager.active_password()
            ),
        });
    } else {
        let _ = ui_tx.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: "SU fixed password loaded from password.yaml".to_string(),
        });
    }
    let _ = ui_tx.send(tui::UiEvent::Log {
        level: tui::UiLevel::Info,
        message: "TUI defaults to SU mode; external sessions need /su <password>.".to_string(),
    });
    let external_gateway_for_handler = external_gateway.clone();

    let mut bot = LiteyukiBot::builder(APP_TITLE, env!("CARGO_PKG_VERSION"))
        .with_target(target)
        .with_runtime_config(runtime_config)
        .with_adapter_configs(adapter_configs.clone())
        .with_adapter_autostart(adapter_autostart)
        .with_plugin_dirs([PathBuf::from(BUILTIN_PLUGIN_DIR)])
        .with_event_handler(move |event, _logger| {
            let ui_tx_for_handler = ui_tx_for_handler.clone();
            let external_gateway_for_handler = external_gateway_for_handler.clone();
            async move {
                let snapshot = external_gateway_for_handler
                    .observe_payload(&event.payload, EXTERNAL_API_TIMEOUT);
                emit_external_stats(&ui_tx_for_handler, &snapshot);
                if should_hide_event_from_tui(&event) {
                    return;
                }
                let _ = ui_tx_for_handler.send(tui::UiEvent::RuntimeHandled {
                    id: event.id,
                    topic: event.topic.clone(),
                    payload_preview: payload_preview(&event.payload),
                });
            }
        })
        .build();
    if let Err(err) = bot
        .plugin_sdk()
        .sync_disabled_scope_commands(&disabled_commands)
    {
        eprintln!("failed to apply persisted command policy at startup: {err}");
    }
    bot.set_disabled_plugin_ids(disabled_plugins.clone());

    emit_external_stats(&ui_tx, &external_gateway.snapshot());
    let external_gateway_for_tick = external_gateway.clone();
    let ui_tx_for_tick = ui_tx.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            let snapshot = external_gateway_for_tick.sweep_timeouts(EXTERNAL_API_TIMEOUT);
            emit_external_stats(&ui_tx_for_tick, &snapshot);
        }
    });

    let tx_before = ui_tx.clone();
    bot.on_before_start_sync("tui-before-start", Default::default(), move |_context| {
        let _ = tx_before.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: "runtime preparing...".to_string(),
        });
        Ok(())
    });

    let tx_after = ui_tx.clone();
    bot.on_after_start_sync("tui-after-start", Default::default(), move |_context| {
        let _ = tx_after.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: "runtime started".to_string(),
        });
        Ok(())
    });

    let tx_before_shutdown = ui_tx.clone();
    bot.on_before_process_shutdown_sync(
        "tui-before-shutdown",
        Default::default(),
        move |_context, process_name| {
            let _ = tx_before_shutdown.send(tui::UiEvent::Log {
                level: tui::UiLevel::Warn,
                message: format!("shutting down process: {}", process_name),
            });
            Ok(())
        },
    );

    install_external_event_handlers(
        &bot,
        external_gateway.clone(),
        ui_tx.clone(),
        help_whitelist.clone(),
        llm_runtime.clone(),
        bot.plugin_sdk().clone(),
        superuser_manager.clone(),
    );

    bot.start().await?;

    let tui_result = tui::run(
        &bot,
        tui::RunOptions {
            target,
            settings_desc: describe_runtime_config(&effective_runtime_config),
            adapter_configs,
            adapter_autostart,
            tui_config,
            reload_handler: reload_from_config,
            whitelist_persist_handler: persist_help_whitelist,
            disabled_commands_persist_handler: persist_disabled_commands_config,
            disabled_plugins_persist_handler: persist_disabled_plugins_config,
            llm_command_handler: handle_llm_tui_command,
            ask_handler: handle_tui_ask_command,
            help_whitelist: help_whitelist.clone(),
            llm_command_prefix: llm_runtime.shared_command_prefix(),
            plugin_sdk: bot.plugin_sdk().clone(),
            plugin_manager: bot.plugin_manager().clone(),
            disabled_plugins,
        },
        &mut ui_rx,
    )
    .await;

    let shutdown_result = bot.shutdown().await;
    if let Err(err) = shutdown_result {
        eprintln!("bot shutdown failed: {err}");
    }

    tui_result?;
    Ok(())
}

fn resolve_runtime_target() -> RuntimeTarget {
    std::env::var("LY_RUNTIME_TARGET")
        .ok()
        .as_deref()
        .and_then(RuntimeTarget::parse)
        .unwrap_or(DEFAULT_RUNTIME_TARGET)
}

fn resolve_password_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_PASSWORD_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    PathBuf::from(PASSWORD_CONFIG_PATH)
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
                warnings.push(format!(
                    "failed to load llm config from {}: {err}",
                    path.display()
                ));
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
    runtime_config: &mut liteyukibot_core::BotRuntimeConfig,
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

fn describe_runtime_config(runtime_config: &liteyukibot_core::BotRuntimeConfig) -> String {
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

fn emit_external_stats(
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
    snapshot: &ExternalGatewaySnapshot,
) {
    let _ = ui_tx.send(tui::UiEvent::ExternalStats {
        command_hits: snapshot.command_hits,
        api_requests: snapshot.api_requests,
        api_success: snapshot.api_success,
        api_failed: snapshot.api_failed,
        api_timeouts: snapshot.api_timeouts,
        api_inflight: snapshot.api_inflight as u64,
    });
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
    format!("用法: {command_prefix} 你的问题")
}

fn command_disabled_text(command_name: &str) -> String {
    format!("命令 '{}' 当前已禁用。", command_name)
}

fn install_external_event_handlers(
    bot: &LiteyukiBot,
    gateway: ExternalGateway,
    ui_tx: mpsc::UnboundedSender<tui::UiEvent>,
    help_whitelist: Arc<RwLock<HashSet<String>>>,
    llm_runtime: LlmCommandRuntime,
    plugin_sdk: PluginSdk,
    superuser_manager: SuperuserManager,
) {
    let adapter_manager = bot.adapter_manager().clone();
    let gateway_for_su = gateway.clone();
    let ui_tx_for_su = ui_tx.clone();
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
            let ui_tx = ui_tx_for_su.clone();
            let superuser_manager = superuser_for_su.clone();
            let plugin_sdk = plugin_sdk_for_su.clone();
            async move {
                handle_external_su_command(
                    &adapter_manager,
                    &gateway,
                    &ui_tx,
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
    let ui_tx_for_help = ui_tx.clone();
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
            let ui_tx = ui_tx_for_help.clone();
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
                        &ui_tx,
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
                        &ui_tx,
                        event.as_ref(),
                        "当前会话未进入 SU 模式，请先发送 /su <password> 完成认证。",
                        "su-required-help",
                    )
                    .await;
                }
                let debug_mode = whitelist_debug_enabled();
                let (allowed, matched_entry, whitelist_size) = help_whitelist
                    .read()
                    .map(|set| {
                        let matched = matched_help_whitelist_entry(event.as_ref(), &set);
                        let allowed = is_help_session_allowed(event.as_ref(), &set);
                        (allowed, matched, set.len())
                    })
                    .unwrap_or_else(|_| (false, None, 0));
                if debug_mode {
                    let _ = ui_tx.send(tui::UiEvent::Log {
                        level: tui::UiLevel::Info,
                        message: format!(
                            "[debug.whitelist] /help text={:?} scope={:?} session={} user={} whitelist_size={} matched={:?} allowed={}",
                            event.message.as_ref(),
                            event.scope,
                            event.session_id.as_ref(),
                            event.user_id.as_ref(),
                            whitelist_size,
                            matched_entry,
                            allowed
                        ),
                    });
                }
                if !allowed {
                    return Ok(());
                }
                reply_help_command(
                    &adapter_manager,
                    &gateway,
                    &ui_tx,
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
    let ui_tx_for_ask = ui_tx.clone();
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
            let ui_tx = ui_tx_for_ask.clone();
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
                        &ui_tx,
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
                        &ui_tx,
                        event.as_ref(),
                        "当前会话未进入 SU 模式，请先发送 /su <password> 完成认证。",
                        "su-required-ask",
                    )
                    .await;
                }
                reply_ask_command(&adapter_manager, &gateway, &ui_tx, &llm_runtime, event).await
            }
        },
    );
}

async fn handle_external_su_command(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
    plugin_sdk: &PluginSdk,
    superuser_manager: &SuperuserManager,
    event: Arc<SessionEvent>,
) -> Result<(), String> {
    let Some(password_raw) = parse_su_password_argument(event.message.as_ref()) else {
        return Ok(());
    };
    if plugin_sdk.is_builtin_command_disabled("adapter:onebot11", "/su") {
        let text = command_disabled_text("/su");
        return reply_external_text(
            adapter_manager,
            gateway,
            ui_tx,
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
            ui_tx,
            event.as_ref(),
            "出于安全考虑，OneBot 的 /su 仅允许私聊发送。",
            "su-private-only",
        )
        .await;
    }

    if password_raw.trim().is_empty() {
        return reply_external_text(
            adapter_manager,
            gateway,
            ui_tx,
            event.as_ref(),
            "用法: /su <password>",
            "su-usage",
        )
        .await;
    }

    if !superuser_manager.verify_password(password_raw.as_str()) {
        return reply_external_text(
            adapter_manager,
            gateway,
            ui_tx,
            event.as_ref(),
            "SU 认证失败：密码错误。",
            "su-denied",
        )
        .await;
    }

    let promoted = superuser_manager
        .promote_user(event.as_ref())
        .map_err(|err| format!("failed to persist superuser: {err}"))?;
    let text = if promoted.added {
        "SU 模式已启用，你已被加入 superuser 列表。"
    } else {
        "SU 模式已启用。"
    };
    reply_external_text(
        adapter_manager,
        gateway,
        ui_tx,
        event.as_ref(),
        text,
        "su-granted",
    )
    .await
}

async fn reply_help_command(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
    llm_runtime: &LlmCommandRuntime,
    plugin_sdk: &PluginSdk,
    event: Arc<SessionEvent>,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    let help_text = render_external_help_text_with_plugins(
        llm_runtime.command_prefix().as_str(),
        Some(plugin_sdk),
    );
    reply_external_text(
        adapter_manager,
        gateway,
        ui_tx,
        event.as_ref(),
        help_text.as_str(),
        "liteyuki-help",
    )
    .await
}

async fn reply_ask_command(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
    llm_runtime: &LlmCommandRuntime,
    event: Arc<SessionEvent>,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    emit_external_stats(ui_tx, &gateway.record_command_hit());

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
                    "LLM 返回空内容".to_string()
                } else {
                    output
                }
            }
            Err(err) => format!("LLM 调用失败: {err}"),
        }
    };

    let echo = gateway.next_echo("liteyuki-ask");
    let payload = build_onebot_v11_text_reply_payload(event.as_ref(), &echo, &reply_text)
        .ok_or_else(|| "failed to build onebot v11 ask response".to_string())?;
    dispatch_onebot_reply(
        adapter_manager,
        gateway,
        ui_tx,
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
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
    event: &SessionEvent,
    text: &str,
    echo_prefix: &str,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    emit_external_stats(ui_tx, &gateway.record_command_hit());
    let echo = gateway.next_echo(echo_prefix);
    let payload = build_onebot_v11_text_reply_payload(event, &echo, text)
        .ok_or_else(|| "failed to build onebot v11 text response".to_string())?;
    dispatch_onebot_reply(
        adapter_manager,
        gateway,
        ui_tx,
        event,
        format!("{echo_prefix}-{}", event.event_id),
        echo,
        payload,
    )
    .await
}

async fn generate_llm_reply(prompt: &str) -> Result<String, String> {
    let llm_config = current_llm_runtime_config()?;
    if !llm_config.enabled {
        return Err("LLM 当前未启用，请在 TUI 输入 /llm on openai".to_string());
    }
    if !llm_config.provider.eq_ignore_ascii_case("openai") {
        return Err(format!(
            "provider '{}' 暂未内建，仅支持 openai 兼容接口",
            llm_config.provider
        ));
    }
    let Some(api_key) = pick_next_api_key(&llm_config) else {
        return Err("缺少 API Key，请在 TUI 输入 /llm apikey <key>".to_string());
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

async fn dispatch_onebot_reply(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
    event: &SessionEvent,
    packet_id: String,
    echo: String,
    payload: Value,
) -> Result<(), String> {
    let adapter_id = event
        .payload
        .get("_adapter_id")
        .and_then(value_to_string)
        .ok_or_else(|| "missing adapter id in inbound payload".to_string())?;

    emit_external_stats(ui_tx, &gateway.track_request(echo.clone()));
    let packet = AdapterPacket::new(packet_id, "onebot.v11.api.send_msg", payload);
    let send_result = adapter_manager.send(&adapter_id, packet).await;
    if let Err(err) = send_result {
        emit_external_stats(ui_tx, &gateway.mark_send_failed(&echo));
        return Err(format!("send reply failed: {err}"));
    }
    Ok(())
}

fn reload_from_config(bot: &LiteyukiBot) -> tui::ReloadFuture<'_> {
    Box::pin(async move {
        let (app_config, mut warnings) = load_app_config_with_llm_overlay();
        warnings.extend(collect_runtime_reload_warnings(&app_config));
        let adapters = load_adapter_configs(&app_config)
            .map_err(|err| format!("failed to load adapter configs: {err}"))?;
        let autostart = !adapters.is_empty();
        bot.reload_adapters(adapters.clone(), autostart)
            .await
            .map_err(|err| format!("failed to apply adapter reload: {err}"))?;
        let tui_config = resolve_tui_config(&app_config);
        let llm_command_prefix = resolve_llm_config(&app_config).command_prefix;
        let disabled_commands = resolve_disabled_scope_commands(&app_config);
        let disabled_plugins = resolve_disabled_plugins(&app_config);
        let mut help_whitelist: Vec<String> =
            resolve_help_whitelist(&app_config).into_iter().collect();
        help_whitelist.sort();
        bot.reload_plugins(disabled_plugins.clone())
            .await
            .map_err(|err| format!("failed to apply plugin reload: {err}"))?;
        Ok(tui::ReloadResult {
            adapters,
            adapter_autostart: autostart,
            tui_config,
            help_whitelist,
            llm_command_prefix,
            disabled_commands,
            disabled_plugins,
            warnings,
        })
    })
}

fn persist_help_whitelist(entries: Vec<String>) -> Result<String, String> {
    let path = resolve_app_config_path().unwrap_or_else(|| PathBuf::from("config.yaml"));
    write_default_config_if_missing(path.as_path())
        .map_err(|err| format!("failed to ensure config exists: {err}"))?;
    config_edit::persist_onebot_v11_whitelist(path.as_path(), &entries)?;
    Ok(format!(
        "whitelist persisted to {} (entries={})",
        path.display(),
        entries.len()
    ))
}

fn persist_disabled_commands_config(entries: Vec<String>) -> Result<String, String> {
    let path = resolve_app_config_path().unwrap_or_else(|| PathBuf::from("config.yaml"));
    write_default_config_if_missing(path.as_path())
        .map_err(|err| format!("failed to ensure config exists: {err}"))?;
    config_edit::persist_disabled_commands(path.as_path(), &entries)?;
    Ok(format!(
        "command policy persisted to {} (disabled={})",
        path.display(),
        entries.len()
    ))
}

fn persist_disabled_plugins_config(entries: Vec<String>) -> Result<String, String> {
    let path = resolve_app_config_path().unwrap_or_else(|| PathBuf::from("config.yaml"));
    write_default_config_if_missing(path.as_path())
        .map_err(|err| format!("failed to ensure config exists: {err}"))?;
    config_edit::persist_disabled_plugins(path.as_path(), &entries)?;
    Ok(format!(
        "plugin policy persisted to {} (disabled={})",
        path.display(),
        entries.len()
    ))
}

fn handle_llm_tui_command(action: tui::LlmCommandRequest) -> tui::LlmCommandFuture<'static> {
    Box::pin(async move {
        match action {
            tui::LlmCommandRequest::SetModel(model) => {
                let patch = config_edit::LlmConfigPatch {
                    model: Some(model.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(format!(
                    "llm.model updated to '{model}' ({})",
                    path.display()
                ))
            }
            tui::LlmCommandRequest::AddApiKeys(new_keys) => {
                let doc = load_current_app_config_doc()?;
                let mut merged = extract_llm_keys_from_doc(&doc);
                merged.extend(new_keys);
                merged = normalize_string_list(merged);
                if merged.is_empty() {
                    return Err("no valid api key provided".to_string());
                }

                let patch = config_edit::LlmConfigPatch {
                    api_keys: Some(merged.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(format!(
                    "llm.api_keys updated (count={}) ({})",
                    merged.len(),
                    path.display()
                ))
            }
            tui::LlmCommandRequest::ProbeProvider(provider_override) => {
                let mut llm_config = current_llm_runtime_config()?;
                if let Some(provider) = provider_override {
                    llm_config.provider = provider;
                }
                let message = probe_llm_configuration(&llm_config).await?;
                Ok(message)
            }
            tui::LlmCommandRequest::AddProviderUrl(provider_url) => {
                let doc = load_current_app_config_doc()?;
                let mut provider_urls = extract_llm_provider_urls_from_doc(&doc);
                if provider_urls.iter().any(|value| value == &provider_url) {
                    return Ok(format!(
                        "llm provider base-url already exists: {}",
                        provider_url
                    ));
                }

                provider_urls.push(provider_url.clone());
                provider_urls = normalize_provider_url_list(provider_urls);
                let active_base_url = configured_llm_base_url_from_doc(&doc)
                    .or_else(|| provider_urls.first().cloned())
                    .unwrap_or_else(|| DEFAULT_LLM_PROVIDER_BASE_URL.to_string());
                let patch = config_edit::LlmConfigPatch {
                    base_url: Some(active_base_url.clone()),
                    provider_urls: Some(provider_urls.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(format!(
                    "llm provider base-url added: {} (count={}, active={}) ({})",
                    provider_url,
                    provider_urls.len(),
                    active_base_url,
                    path.display()
                ))
            }
            tui::LlmCommandRequest::RemoveProviderUrl(provider_url) => {
                let doc = load_current_app_config_doc()?;
                let mut provider_urls = extract_llm_provider_urls_from_doc(&doc);
                ensure_registered_provider_url(provider_url.as_str(), &provider_urls)?;
                provider_urls.retain(|value| value != &provider_url);

                let configured_base_url = configured_llm_base_url_from_doc(&doc);
                let active_base_url = if configured_base_url
                    .as_ref()
                    .is_some_and(|base| base == &provider_url)
                {
                    provider_urls.first().cloned()
                } else {
                    configured_base_url.or_else(|| provider_urls.first().cloned())
                }
                .unwrap_or_else(|| DEFAULT_LLM_PROVIDER_BASE_URL.to_string());

                let patch = config_edit::LlmConfigPatch {
                    base_url: Some(active_base_url.clone()),
                    provider_urls: Some(provider_urls.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(format!(
                    "llm provider base-url removed: {} (count={}, active={}) ({})",
                    provider_url,
                    provider_urls.len(),
                    active_base_url,
                    path.display()
                ))
            }
            tui::LlmCommandRequest::ListProviderUrls => {
                let doc = load_current_app_config_doc()?;
                let llm_config = current_llm_runtime_config()?;
                let provider_urls = extract_llm_provider_urls_from_doc(&doc);
                let path = resolve_llm_config_path().unwrap_or_else(resolve_llm_config_write_path);
                let mut lines = vec![format!("* {}", llm_config.base_url)];
                lines.extend(
                    provider_urls
                        .iter()
                        .filter(|url| **url != llm_config.base_url)
                        .map(|url| format!("  {url}")),
                );
                Ok(format!(
                    "llm provider base-url list ({})\n{}",
                    path.display(),
                    lines.join("\n")
                ))
            }
            tui::LlmCommandRequest::UseProviderUrl(provider_url) => {
                let doc = load_current_app_config_doc()?;
                let provider_urls = extract_llm_provider_urls_from_doc(&doc);
                ensure_registered_provider_url(provider_url.as_str(), &provider_urls)?;

                let patch = config_edit::LlmConfigPatch {
                    base_url: Some(provider_url.clone()),
                    provider_urls: Some(provider_urls.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(format!(
                    "llm provider base-url switched: {} (count={}) ({})",
                    provider_url,
                    provider_urls.len(),
                    path.display()
                ))
            }
            tui::LlmCommandRequest::SetEnabled { enabled, provider } => {
                let patch = config_edit::LlmConfigPatch {
                    enabled: Some(enabled),
                    provider,
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(format!(
                    "llm {} ({})",
                    if enabled { "enabled" } else { "disabled" },
                    path.display()
                ))
            }
            tui::LlmCommandRequest::PromptList => {
                let store = load_llm_prompt_store()?;
                let path = resolve_llm_prompt_store_path();
                let mut names = store.profile_names();
                names.sort();

                let lines = names
                    .into_iter()
                    .map(|name| {
                        if name == store.active_profile {
                            format!("* {name}")
                        } else {
                            format!("  {name}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                Ok(format!(
                    "llm prompt profiles ({})\n{}",
                    path.display(),
                    lines
                ))
            }
            tui::LlmCommandRequest::PromptUse(name) => {
                let mut store = load_llm_prompt_store()?;
                store.set_active_profile(name.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(format!(
                    "llm active prompt profile -> '{}' ({})",
                    store.active_profile,
                    path.display()
                ))
            }
            tui::LlmCommandRequest::PromptSet { name, soul } => {
                let mut store = load_llm_prompt_store()?;
                store.upsert_profile(name.as_str(), soul.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(format!(
                    "llm prompt profile '{}' updated ({})",
                    name,
                    path.display()
                ))
            }
            tui::LlmCommandRequest::PromptRemove(name) => {
                let mut store = load_llm_prompt_store()?;
                store.remove_profile(name.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(format!(
                    "llm prompt profile '{}' removed ({})",
                    name,
                    path.display()
                ))
            }
            tui::LlmCommandRequest::PromptPreview { user_prompt } => {
                let llm_config = current_llm_runtime_config()?;
                let profile = current_active_prompt_profile()?;
                let preview = build_prompt_preview(
                    llm_config.system_prompt.as_deref(),
                    user_prompt.as_str(),
                    profile.soul.as_str(),
                );
                Ok(format_prompt_preview(&profile.name, &preview))
            }
        }
    })
}

fn handle_tui_ask_command(prompt: String) -> tui::AskFuture<'static> {
    Box::pin(async move {
        let output = generate_llm_reply(&prompt).await?;
        if output.trim().is_empty() {
            Ok("LLM 返回空内容".to_string())
        } else {
            Ok(output)
        }
    })
}

fn persist_llm_patch(patch: &config_edit::LlmConfigPatch) -> Result<PathBuf, String> {
    let path = resolve_llm_config_write_path();
    ensure_llm_config_file(path.as_path())?;
    config_edit::persist_llm_config(path.as_path(), patch)?;
    Ok(path)
}

fn current_llm_runtime_config() -> Result<LlmRuntimeConfig, String> {
    let doc = load_current_app_config_doc()?;
    Ok(resolve_llm_config(&doc))
}

fn load_current_app_config_doc() -> Result<AppConfigDoc, String> {
    let (doc, _) = load_app_config_with_llm_overlay();
    Ok(doc)
}

fn resolve_llm_config_write_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_LLM_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    PathBuf::from(LLM_CONFIG_PATHS[0])
}

fn resolve_llm_prompt_store_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_LLM_PROMPT_STORE_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    PathBuf::from(LLM_PROMPT_STORE_PATH)
}

fn load_llm_prompt_store() -> Result<LlmPromptStore, String> {
    let path = resolve_llm_prompt_store_path();
    LlmPromptStore::load_or_default_from_path(path.as_path())
}

fn persist_llm_prompt_store(store: &LlmPromptStore) -> Result<PathBuf, String> {
    let path = resolve_llm_prompt_store_path();
    store.save_to_path(path.as_path())?;
    Ok(path)
}

fn current_active_prompt_profile() -> Result<LlmPromptProfile, String> {
    let store = load_llm_prompt_store()?;
    Ok(store.active_profile())
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

fn extract_llm_keys_from_doc(doc: &AppConfigDoc) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(section) = doc.llm.as_ref() {
        if let Some(api_keys) = section.api_keys.as_ref() {
            keys.extend(api_keys.iter().cloned());
        }
        if let Some(api_key) = section.api_key.as_deref() {
            keys.push(api_key.to_string());
        }
    }
    normalize_string_list(keys)
}

fn extract_llm_provider_urls_from_doc(doc: &AppConfigDoc) -> Vec<String> {
    doc.llm
        .as_ref()
        .and_then(|section| section.provider_urls.clone())
        .map(normalize_provider_url_list)
        .unwrap_or_default()
}

fn configured_llm_base_url_from_doc(doc: &AppConfigDoc) -> Option<String> {
    doc.llm
        .as_ref()
        .and_then(|section| section.base_url.as_deref())
        .and_then(normalize_provider_url)
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

fn normalize_provider_url(raw: &str) -> Option<String> {
    let value = raw.trim().trim_end_matches('/').to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn normalize_provider_url_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for value in values {
        let Some(value) = normalize_provider_url(value.as_str()) else {
            continue;
        };
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

fn ensure_registered_provider_url(
    provider_url: &str,
    provider_urls: &[String],
) -> Result<(), String> {
    if provider_urls.is_empty() {
        return Err(
            "no provider base-url configured, run /llm provider add <base-url>".to_string(),
        );
    }
    if provider_urls.iter().all(|value| value != provider_url) {
        return Err(format!("llm provider base-url not found: {}", provider_url));
    }
    Ok(())
}

async fn probe_llm_configuration(llm_config: &LlmRuntimeConfig) -> Result<String, String> {
    if !llm_config.provider.eq_ignore_ascii_case("openai") {
        return Err(format!(
            "provider '{}' 暂未内建，仅支持 openai 兼容接口",
            llm_config.provider
        ));
    }
    let Some(api_key) = llm_config.api_keys.first() else {
        return Ok(format!(
            "llm probe skipped remote request: provider={} base_url={} (no api key configured)",
            llm_config.provider, llm_config.base_url
        ));
    };

    let client = OpenAiResponsesClient::from_runtime_with_api_key(llm_config, api_key)
        .map_err(|err| err.to_string())?;
    let output = client
        .generate("Reply exactly with: OK")
        .await
        .map_err(|err| err.to_string())?;
    let preview = truncate_text_for_log(output.trim(), 80);
    Ok(format!(
        "llm probe success: provider={} model={} output={}",
        llm_config.provider, llm_config.model, preview
    ))
}

fn format_prompt_preview(profile_name: &str, preview: &LlmPromptPreview) -> String {
    let system_prompt = if preview.system_prompt.trim().is_empty() {
        "<empty>".to_string()
    } else {
        preview.system_prompt.clone()
    };
    let composed_user_prompt = if preview.composed_user_prompt.trim().is_empty() {
        "<empty>".to_string()
    } else {
        preview.composed_user_prompt.clone()
    };

    format!(
        "active profile: {profile_name}\nsystem prompt:\n{system_prompt}\n\ncomposed user prompt:\n{composed_user_prompt}\n\ncombined prompt preview:\n{}",
        preview.combined_prompt
    )
}

fn truncate_text_for_log(raw: &str, max_chars: usize) -> String {
    let mut iter = raw.chars();
    let preview: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn llm_config_env_lock() -> &'static Mutex<()> {
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

    fn temp_llm_config_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("rsliteyukibot-{name}-{unique}.yaml"))
    }

    fn run_llm_command_for_test(action: tui::LlmCommandRequest) -> Result<String, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(handle_llm_tui_command(action))
    }

    #[test]
    fn llm_provider_use_rejects_unregistered_base_url_without_mutating_config() {
        let _lock = llm_config_env_lock()
            .lock()
            .expect("llm config env lock should not be poisoned");
        let path = temp_llm_config_path("provider-use-invalid");
        let source = "llm:\n  base_url: https://api.openai.com\n  provider_urls:\n    - https://api.openai.com\n    - https://tokenflux.dev/v1\n";
        fs::write(&path, source).expect("test llm config should be written");
        let _env_guard = EnvVarGuard::set("LY_LLM_CONFIG_PATH", path.as_path());

        let result = run_llm_command_for_test(tui::LlmCommandRequest::UseProviderUrl(
            "https://typo.example/v1".to_string(),
        ));

        assert_eq!(
            result,
            Err("llm provider base-url not found: https://typo.example/v1".to_string())
        );
        let updated = fs::read_to_string(&path).expect("test llm config should remain readable");
        assert_eq!(updated, source);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn llm_provider_use_switches_to_registered_base_url() {
        let _lock = llm_config_env_lock()
            .lock()
            .expect("llm config env lock should not be poisoned");
        let path = temp_llm_config_path("provider-use-valid");
        let source = "llm:\n  base_url: https://api.openai.com\n  provider_urls:\n    - https://api.openai.com\n    - https://tokenflux.dev/v1\n";
        fs::write(&path, source).expect("test llm config should be written");
        let _env_guard = EnvVarGuard::set("LY_LLM_CONFIG_PATH", path.as_path());

        let result = run_llm_command_for_test(tui::LlmCommandRequest::UseProviderUrl(
            "https://tokenflux.dev/v1".to_string(),
        ))
        .expect("registered provider url should switch successfully");

        assert!(result.contains("llm provider base-url switched: https://tokenflux.dev/v1"));
        let updated = fs::read_to_string(&path).expect("updated llm config should be readable");
        assert!(updated.contains("base_url: 'https://tokenflux.dev/v1'"));
        assert!(updated.contains("provider_urls:"));
        assert!(updated.contains("- 'https://api.openai.com'"));
        assert!(updated.contains("- 'https://tokenflux.dev/v1'"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn external_ask_command_prefix_reads_runtime_updates() {
        let llm_runtime = LlmCommandRuntime::new("/ask");

        assert!(matches_external_ask_command(
            "/ask hello world",
            &llm_runtime
        ));
        assert_eq!(
            llm_usage_text(llm_runtime.command_prefix().as_str()),
            "用法: /ask 你的问题"
        );

        llm_runtime.set_command_prefix("/qa");

        assert!(!matches_external_ask_command(
            "/ask hello world",
            &llm_runtime
        ));
        assert!(matches_external_ask_command(
            "/qa hello world",
            &llm_runtime
        ));
        assert_eq!(
            llm_usage_text(llm_runtime.command_prefix().as_str()),
            "用法: /qa 你的问题"
        );
    }
}
