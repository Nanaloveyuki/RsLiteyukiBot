use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use liteyukibot_core::adapter::AdapterManager;
use liteyukibot_core::session::SessionEvent;
use liteyukibot_core::{
    AdapterPacket, LiteyukiBot, LogLevel, Rule, RuntimeSettings, RuntimeTarget,
};
use serde_json::Value;
use tokio::sync::mpsc;

mod app_config;
mod config_edit;
mod onebot_support;
mod tui;

use app_config::*;
use onebot_support::*;

const APP_TITLE: &str = "RsLiteyukiBot";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;
const EXTERNAL_API_TIMEOUT: Duration = Duration::from_secs(12);

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
    let active_settings = RuntimeSettings::global_or_default();
    let mut runtime_config = RuntimeSettings::global_runtime_config().clone();
    runtime_config.logger.min_level = LogLevel::Error;

    let app_config = load_app_config();
    prime_reload_warning_state(&app_config);
    let target = resolve_runtime_target();
    let adapter_configs = load_adapter_configs(&app_config).unwrap_or_default();
    let adapter_autostart = !adapter_configs.is_empty();
    let help_whitelist = Arc::new(RwLock::new(resolve_help_whitelist(&app_config)));
    let tui_config = resolve_tui_config(&app_config);
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
    );

    bot.start().await?;

    let tui_result = tui::run(
        &mut bot,
        tui::RunOptions {
            target,
            settings_desc: active_settings.describe(),
            adapter_configs,
            adapter_autostart,
            tui_config,
            reload_handler: reload_from_config,
            whitelist_persist_handler: persist_help_whitelist,
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
) {
    let adapter_manager = bot.adapter_manager().clone();

    bot.on_message(
        "builtin.external.help",
        Rule::new("command.help", |event| async move {
            is_help_command(event.message.as_ref())
        }),
        500,
        true,
        move |event| {
            let adapter_manager = adapter_manager.clone();
            let gateway = gateway.clone();
            let ui_tx = ui_tx.clone();
            let help_whitelist = help_whitelist.clone();
            async move {
                let allowed = help_whitelist
                    .read()
                    .map(|set| is_help_session_allowed(event.as_ref(), &set))
                    .unwrap_or(false);
                if !allowed {
                    return Ok(());
                }
                reply_help_command(&adapter_manager, &gateway, &ui_tx, event).await
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

    let adapter_id = event
        .payload
        .get("_adapter_id")
        .and_then(value_to_string)
        .ok_or_else(|| "missing adapter id in inbound payload".to_string())?;

    let echo = gateway.next_echo("liteyuki-help");
    let payload = build_onebot_v11_help_reply_payload(event.as_ref(), &echo)
        .ok_or_else(|| "failed to build onebot v11 help response".to_string())?;
    emit_external_stats(ui_tx, &gateway.track_request(echo.clone()));

    let packet = AdapterPacket::new(
        format!("help-{}", event.event_id),
        "onebot.v11.api.send_msg",
        payload,
    );

    let send_result = adapter_manager.send(&adapter_id, packet).await;

    if let Err(err) = send_result {
        emit_external_stats(ui_tx, &gateway.mark_send_failed(&echo));
        return Err(format!("send help reply failed: {err}"));
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
