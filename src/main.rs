pub(crate) use liteyukibot_core::{BotEvent, PluginSdk, SessionEvent, SessionScope};
use liteyukibot_core::{LiteyukiBot, RuntimeTarget};
use tokio::sync::mpsc;

mod app_config;
mod command_registry;
mod config_edit;
mod external_commands;
mod flow_local_agent;
// 外部调用
#[allow(dead_code)]
mod hardcode_data;
mod i18n;
mod llm;
mod main_support;
mod onebot_support;
mod runtime_support;
mod superuser;
mod tui;
// 外部调用
#[allow(dead_code)]
mod utils;

use crate::external_commands::install_external_event_handlers;
use crate::main_support::{
    TuiExternalCommandObserver, apply_plugin_policy, emit_external_stats, handle_llm_tui_command,
    handle_tui_ask_command, install_tui_lifecycle_hooks, persist_disabled_commands_config,
    persist_disabled_plugins_config, persist_help_whitelist, reload_from_config,
    send_startup_ui_logs, spawn_external_stats_ticker,
};
use crate::runtime_support::{
    EXTERNAL_API_TIMEOUT, describe_runtime_config, prepare_runtime_bootstrap,
};
#[cfg(test)]
use crate::runtime_support::{
    push_explicit_plugin_dir_candidates, push_runtime_plugin_dir_candidates,
};
use i18n::trf;
use onebot_support::*;
#[cfg(test)]
use std::collections::HashSet;
#[cfg(test)]
use std::path::PathBuf;

const APP_TITLE: &str = "Liteyuki";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = resolve_runtime_target();
    let bootstrap = prepare_runtime_bootstrap(target, |warning| eprintln!("{warning}"))?;
    for warning in &bootstrap.warnings {
        eprintln!("{warning}");
    }
    let tui_config = bootstrap.tui_config;
    let effective_runtime_config = bootstrap.effective_runtime_config.clone();
    let runtime_config = bootstrap.runtime_config;
    let adapter_configs = bootstrap.adapter_configs;
    let adapter_autostart = bootstrap.adapter_autostart;
    let help_whitelist = bootstrap.help_whitelist;
    let llm_runtime = bootstrap.llm_runtime;
    let flow_local_agent = bootstrap.flow_local_agent;
    let external_gateway = bootstrap.external_gateway;
    let plugin_dirs = bootstrap.plugin_dirs;
    let disabled_commands = bootstrap.disabled_commands;
    let disabled_plugins = bootstrap.disabled_plugins;
    let superuser_manager = bootstrap.superuser_manager;

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<tui::UiEvent>();
    let ui_tx_for_handler = ui_tx.clone();
    send_startup_ui_logs(&ui_tx, &help_whitelist, &llm_runtime, &superuser_manager);
    let external_gateway_for_handler = external_gateway.clone();

    let mut bot = LiteyukiBot::builder(APP_TITLE, env!("CARGO_PKG_VERSION"))
        .with_target(target)
        .with_runtime_config(runtime_config)
        .with_adapter_configs(adapter_configs.clone())
        .with_adapter_autostart(adapter_autostart)
        .with_plugin_dirs(plugin_dirs.clone())
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
        eprintln!(
            "{}",
            trf(
                "startup.command_policy_sync_failed",
                &[("err", err.to_string().as_str())],
            )
        );
    }
    bot.set_disabled_plugin_ids(disabled_plugins.clone());

    emit_external_stats(&ui_tx, &external_gateway.snapshot());
    spawn_external_stats_ticker(external_gateway.clone(), ui_tx.clone());
    install_tui_lifecycle_hooks(&mut bot, &ui_tx);

    install_external_event_handlers(
        &bot,
        external_gateway.clone(),
        TuiExternalCommandObserver::new(ui_tx.clone()),
        help_whitelist.clone(),
        llm_runtime.clone(),
        bot.plugin_sdk().clone(),
        superuser_manager.clone(),
    );

    bot.start().await?;
    flow_local_agent.client.spawn_background();

    let tui_result = tui::run(
        &bot,
        tui::RunOptions {
            target,
            settings_desc: describe_runtime_config(&effective_runtime_config),
            adapter_configs,
            adapter_autostart,
            tui_config,
            reload_handler: reload_from_config,
            plugin_policy_handler: apply_plugin_policy,
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
        eprintln!(
            "{}",
            trf(
                "startup.bot_shutdown_failed",
                &[("err", err.to_string().as_str())]
            )
        );
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

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
