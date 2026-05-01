use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::Utc;
use tokio::sync::Mutex as AsyncMutex;

use crate::external_commands::{ExternalCommandObserver, install_external_event_handlers};
use crate::i18n::{tr, trf};
use crate::onebot_support::{payload_preview, should_hide_event_from_tui};
use crate::runtime_support::{
    EXTERNAL_API_TIMEOUT, ExternalGatewaySnapshot, describe_runtime_config,
    prepare_runtime_bootstrap,
};
use crate::{AdapterConfig, LiteyukiBot, LogLevel, RuntimeTarget, emit_console_log};

use super::{
    APP_TITLE, AppHostState, AppHostStateSnapshot, EmbeddedAppHost, resource_usage,
    runtime_target_name, with_state_write,
};

const PLUGIN_CRON_SAMPLE_INTERVAL: Duration = Duration::from_secs(15);

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
        let flow_local_agent = bootstrap.flow_local_agent;
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
            external_stats: Default::default(),
            resource_usage: Default::default(),
        })));
        with_state_write(&state, |host| {
            host.push_note(format!(
                "bootstrapping target={} adapters={} plugin_dirs={}",
                runtime_target_name(target),
                adapter_configs.len(),
                plugin_dirs.len()
            ));
            host.push_note(format!(
                "flow local agent initialized (enabled={})",
                flow_local_agent.config_snapshot().enabled
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
                    update_external_stats(&state, &snapshot);
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

        update_external_stats(&state, &external_gateway.snapshot());
        resource_usage::spawn_resource_usage_sampler(state.clone());
        spawn_external_stats_ticker(state.clone(), external_gateway.clone());
        install_embedded_lifecycle_hooks(&mut bot, state.clone());

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
        flow_local_agent.spawn_background();
        with_state_write(&state, |host| {
            host.set_status("running");
            host.push_note("embedded runtime ready");
        });

        let bot = Arc::new(AsyncMutex::new(bot));
        spawn_plugin_cron_scheduler(bot.clone(), state.clone());

        Ok(Self {
            bot,
            state,
            flow_local_agent,
        })
    }
}

pub(crate) async fn shutdown_embedded_app_host(host: &EmbeddedAppHost) -> Result<(), String> {
    with_state_write(&host.state, |state| {
        state.set_status("stopping");
        state.push_note("shutdown requested");
    });

    let mut bot = host.bot.lock().await;
    match bot.shutdown().await {
        Ok(()) => {
            with_state_write(&host.state, |state| {
                state.set_status("stopped");
                state.push_note("embedded runtime stopped");
            });
            Ok(())
        }
        Err(err) => Err(format!("failed to stop embedded app host: {err}")),
    }
}

pub(crate) async fn apply_disabled_plugins(
    host: &EmbeddedAppHost,
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
    let bot = host.bot.lock().await;
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
    with_state_write(&host.state, |state| {
        state.snapshot.disabled_plugins = disabled_plugin_ids.clone();
        state.push_note(format!(
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

pub(crate) async fn apply_adapter_configs(
    host: &EmbeddedAppHost,
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
    let bot = host.bot.lock().await;
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
    with_state_write(&host.state, |state| {
        state.snapshot.adapter_count = adapter_configs.len();
        state.snapshot.adapter_autostart = autostart;
        state.push_note(format!(
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

fn install_embedded_lifecycle_hooks(bot: &mut LiteyukiBot, state: Arc<RwLock<AppHostState>>) {
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
}

fn spawn_external_stats_ticker(
    state: Arc<RwLock<AppHostState>>,
    external_gateway: crate::runtime_support::ExternalGateway,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            let snapshot = external_gateway.sweep_timeouts(EXTERNAL_API_TIMEOUT);
            update_external_stats(&state, &snapshot);
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
