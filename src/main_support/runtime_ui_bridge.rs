use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::external_commands::ExternalCommandObserver;
use crate::i18n::{tr, trf};
use crate::runtime_support::{
    EXTERNAL_API_TIMEOUT, ExternalGateway, ExternalGatewaySnapshot, LlmCommandRuntime,
};
use crate::superuser::SuperuserManager;
use crate::tui;
use liteyukibot_core::{LiteyukiBot, SessionEvent};
use tokio::sync::mpsc;

pub(crate) fn send_startup_ui_logs(
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
    help_whitelist: &Arc<RwLock<HashSet<String>>>,
    llm_runtime: &LlmCommandRuntime,
    superuser_manager: &SuperuserManager,
) {
    let whitelist_size = help_whitelist
        .read()
        .map(|set| set.len())
        .unwrap_or_default();
    if whitelist_size > 0 {
        let _ = ui_tx.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: trf(
                "startup.whitelist_enabled",
                &[("count", whitelist_size.to_string().as_str())],
            ),
        });
    }
    let _ = ui_tx.send(tui::UiEvent::Log {
        level: tui::UiLevel::Info,
        message: trf(
            "startup.llm_prefix",
            &[("prefix", llm_runtime.command_prefix().as_str())],
        ),
    });
    if superuser_manager.using_dynamic_password() {
        let _ = ui_tx.send(tui::UiEvent::Log {
            level: tui::UiLevel::Warn,
            message: trf(
                "startup.su_dynamic",
                &[("password", superuser_manager.active_password().as_str())],
            ),
        });
    } else {
        let _ = ui_tx.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: tr("startup.su_fixed"),
        });
    }
    let _ = ui_tx.send(tui::UiEvent::Log {
        level: tui::UiLevel::Info,
        message: tr("startup.tui_su_default"),
    });
}

pub(crate) fn emit_external_stats(
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

pub(crate) fn spawn_external_stats_ticker(
    external_gateway: ExternalGateway,
    ui_tx: mpsc::UnboundedSender<tui::UiEvent>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            let snapshot = external_gateway.sweep_timeouts(EXTERNAL_API_TIMEOUT);
            emit_external_stats(&ui_tx, &snapshot);
        }
    });
}

pub(crate) fn install_tui_lifecycle_hooks(
    bot: &mut LiteyukiBot,
    ui_tx: &mpsc::UnboundedSender<tui::UiEvent>,
) {
    let tx_before = ui_tx.clone();
    bot.on_before_start_sync("tui-before-start", Default::default(), move |_context| {
        let _ = tx_before.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: tr("startup.runtime_preparing"),
        });
        Ok(())
    });

    let tx_after = ui_tx.clone();
    bot.on_after_start_sync("tui-after-start", Default::default(), move |_context| {
        let _ = tx_after.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: tr("startup.runtime_started"),
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
                message: trf(
                    "startup.shutting_down_process",
                    &[("process", process_name.as_ref())],
                ),
            });
            Ok(())
        },
    );
}

#[derive(Clone)]
pub(crate) struct TuiExternalCommandObserver {
    ui_tx: mpsc::UnboundedSender<tui::UiEvent>,
}

impl TuiExternalCommandObserver {
    pub(crate) fn new(ui_tx: mpsc::UnboundedSender<tui::UiEvent>) -> Self {
        Self { ui_tx }
    }
}

impl ExternalCommandObserver for TuiExternalCommandObserver {
    fn record_stats(&self, snapshot: &ExternalGatewaySnapshot) {
        emit_external_stats(&self.ui_tx, snapshot);
    }

    fn on_help_whitelist_evaluated(
        &self,
        event: &SessionEvent,
        matched_entry: Option<&str>,
        whitelist_size: usize,
        allowed: bool,
    ) {
        if !crate::onebot_support::whitelist_debug_enabled() {
            return;
        }
        let _ = self.ui_tx.send(tui::UiEvent::Log {
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
}
