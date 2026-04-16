use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use liteyukibot_core::adapter::AdapterManager;
use liteyukibot_core::session::SessionEvent;
use liteyukibot_core::{
    AdapterPacket, LiteyukiBot, LogLevel, LogMode, Rule, RuntimeSettings, RuntimeTarget, TimeZone,
    TimestampFormat,
};
use serde_json::Value;
use tokio::sync::mpsc;

mod app_config;
mod config_edit;
mod llm_client;
mod onebot_support;
mod tui;

use app_config::*;
use llm_client::OpenAiResponsesClient;
use onebot_support::*;

const APP_TITLE: &str = "RsLiteyukiBot";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;
const EXTERNAL_API_TIMEOUT: Duration = Duration::from_secs(12);
const LLM_USAGE_TEXT: &str = "用法: /ask 你的问题";
const LLM_CONFIG_PATHS: [&str; 2] = ["llm-config.yaml", "llm-config.toml"];

static LLM_API_KEY_ROUND_ROBIN: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct LlmCommandRuntime {
    command_prefix: Arc<str>,
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
    let llm_runtime = LlmCommandRuntime {
        command_prefix: Arc::from(llm_config.command_prefix.clone()),
    };
    let external_gateway = ExternalGateway::new();

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
        message: format!("LLM command prefix: {}", llm_runtime.command_prefix),
    });
    let external_gateway_for_handler = external_gateway.clone();

    let mut bot = LiteyukiBot::builder(APP_TITLE, env!("CARGO_PKG_VERSION"))
        .with_target(target)
        .with_runtime_config(runtime_config)
        .with_adapter_configs(adapter_configs.clone())
        .with_adapter_autostart(adapter_autostart)
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
    );

    bot.start().await?;

    let tui_result = tui::run(
        &mut bot,
        tui::RunOptions {
            target,
            settings_desc: describe_runtime_config(&effective_runtime_config),
            adapter_configs,
            adapter_autostart,
            tui_config,
            reload_handler: reload_from_config,
            whitelist_persist_handler: persist_help_whitelist,
            llm_command_handler: handle_llm_tui_command,
            ask_handler: handle_tui_ask_command,
            help_whitelist: help_whitelist.clone(),
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

fn install_external_event_handlers(
    bot: &LiteyukiBot,
    gateway: ExternalGateway,
    ui_tx: mpsc::UnboundedSender<tui::UiEvent>,
    help_whitelist: Arc<RwLock<HashSet<String>>>,
    llm_runtime: LlmCommandRuntime,
) {
    let adapter_manager = bot.adapter_manager().clone();
    let gateway_for_help = gateway.clone();
    let ui_tx_for_help = ui_tx.clone();
    let help_whitelist_for_help = help_whitelist.clone();

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
            async move {
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
                reply_help_command(&adapter_manager, &gateway, &ui_tx, event).await
            }
        },
    );

    let adapter_manager = bot.adapter_manager().clone();
    let gateway_for_ask = gateway.clone();
    let ui_tx_for_ask = ui_tx.clone();
    let command_prefix = llm_runtime.command_prefix.clone();
    bot.on_message(
        "builtin.external.ask",
        Rule::new("command.ask", move |event| {
            let command_prefix = command_prefix.clone();
            async move {
                parse_command_argument(event.message.as_ref(), command_prefix.as_ref()).is_some()
            }
        }),
        490,
        true,
        move |event| {
            let adapter_manager = adapter_manager.clone();
            let gateway = gateway_for_ask.clone();
            let ui_tx = ui_tx_for_ask.clone();
            let llm_runtime = llm_runtime.clone();
            async move {
                reply_ask_command(&adapter_manager, &gateway, &ui_tx, &llm_runtime, event).await
            }
        },
    );
}

async fn reply_help_command(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
    event: Arc<SessionEvent>,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    emit_external_stats(ui_tx, &gateway.record_command_hit());

    let echo = gateway.next_echo("liteyuki-help");
    let payload = build_onebot_v11_help_reply_payload(event.as_ref(), &echo)
        .ok_or_else(|| "failed to build onebot v11 help response".to_string())?;
    dispatch_onebot_reply(
        adapter_manager,
        gateway,
        ui_tx,
        event.as_ref(),
        format!("help-{}", event.event_id),
        echo,
        payload,
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

    let prompt =
        parse_command_argument(event.message.as_ref(), llm_runtime.command_prefix.as_ref())
            .unwrap_or_default();
    let reply_text = if prompt.is_empty() {
        LLM_USAGE_TEXT.to_string()
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

    let client = OpenAiResponsesClient::from_runtime_with_api_key(&llm_config, &api_key)
        .map_err(|err| err.to_string())?;
    client.generate(prompt).await.map_err(|err| err.to_string())
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

fn reload_from_config(bot: &mut LiteyukiBot) -> tui::ReloadFuture<'_> {
    Box::pin(async move {
        let (app_config, mut warnings) = load_app_config_with_warnings(false);
        warnings.extend(collect_runtime_reload_warnings(&app_config));
        let adapters = load_adapter_configs(&app_config)
            .map_err(|err| format!("failed to load adapter configs: {err}"))?;
        let autostart = !adapters.is_empty();
        bot.reload_adapters(adapters.clone(), autostart)
            .await
            .map_err(|err| format!("failed to apply adapter reload: {err}"))?;
        let tui_config = resolve_tui_config(&app_config);
        let mut help_whitelist: Vec<String> =
            resolve_help_whitelist(&app_config).into_iter().collect();
        help_whitelist.sort();
        Ok(tui::ReloadResult {
            adapters,
            adapter_autostart: autostart,
            tui_config,
            help_whitelist,
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
            "[llm]\nenabled = false\nprovider = \"openai\"\nbase_url = \"https://api.openai.com\"\nmodel = \"gpt-4.1-mini\"\ntimeout_seconds = 20\ncommand_prefix = \"/ask\"\napi_keys = []\n"
        }
        _ => {
            "llm:\n  enabled: false\n  provider: openai\n  base_url: https://api.openai.com\n  model: gpt-4.1-mini\n  timeout_seconds: 20\n  command_prefix: /ask\n  api_keys: []\n"
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

async fn probe_llm_configuration(llm_config: &LlmRuntimeConfig) -> Result<String, String> {
    if !llm_config.provider.eq_ignore_ascii_case("openai") {
        return Err(format!(
            "provider '{}' 暂未内建，仅支持 openai 兼容接口",
            llm_config.provider
        ));
    }
    let Some(api_key) = llm_config.api_keys.first() else {
        return Err("no api key configured, run /llm apikey <key>".to_string());
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

fn truncate_text_for_log(raw: &str, max_chars: usize) -> String {
    let mut iter = raw.chars();
    let preview: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}
