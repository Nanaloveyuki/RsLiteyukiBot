use std::path::PathBuf;

use liteyukibot_core::{AdapterConfig, LiteyukiBot, LogLevel, RuntimeSettings, RuntimeTarget};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;

mod tui;

const APP_TITLE: &str = "RsLiteyukiBot";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;

#[derive(Debug, Deserialize)]
struct AdapterConfigDoc {
    adapters: Vec<AdapterConfig>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let target = resolve_runtime_target();
    let adapter_configs = load_adapter_configs().unwrap_or_default();
    let adapter_autostart = !adapter_configs.is_empty();

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<tui::UiEvent>();
    let ui_tx_for_handler = ui_tx.clone();

    let mut bot = LiteyukiBot::builder(APP_TITLE, env!("CARGO_PKG_VERSION"))
        .with_target(target)
        .with_runtime_config(runtime_config)
        .with_adapter_configs(adapter_configs.clone())
        .with_adapter_autostart(adapter_autostart)
        .with_event_handler(move |event, _logger| {
            let ui_tx_for_handler = ui_tx_for_handler.clone();
            async move {
                let _ = ui_tx_for_handler.send(tui::UiEvent::RuntimeHandled {
                    id: event.id,
                    topic: event.topic.clone(),
                    payload_preview: payload_preview(&event.payload),
                });
            }
        })
        .build();

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

    bot.start().await?;

    let tui_result = tui::run(
        &mut bot,
        target,
        active_settings.describe(),
        adapter_configs,
        adapter_autostart,
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

fn load_adapter_configs() -> Result<Vec<AdapterConfig>, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("LY_ADAPTERS_PATH") {
        let content = std::fs::read_to_string(PathBuf::from(path))?;
        if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(&content) {
            return Ok(doc.adapters);
        }
        let list = serde_json::from_str::<Vec<AdapterConfig>>(&content)?;
        return Ok(list);
    }

    if let Ok(raw) = std::env::var("LY_ADAPTERS_JSON") {
        if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(&raw) {
            return Ok(doc.adapters);
        }
        let list = serde_json::from_str::<Vec<AdapterConfig>>(&raw)?;
        return Ok(list);
    }

    Ok(Vec::new())
}

fn payload_preview(payload: &Value) -> String {
    let raw = payload.to_string();
    const MAX: usize = 96;
    let mut iter = raw.chars();
    let preview: String = iter.by_ref().take(MAX).collect();
    if iter.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}
