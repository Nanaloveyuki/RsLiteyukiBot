use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

pub(crate) use liteyukibot_core::{
    BotEvent, PluginManifestLoader, PluginSdk, SessionEvent, SessionScope,
};
use liteyukibot_core::{LiteyukiBot, LogLevel, RuntimeSettings, RuntimeTarget};
use tokio::sync::mpsc;

mod app_config;
mod command_registry;
mod config_edit;
mod external_commands;
mod i18n;
mod llm;
mod onebot_support;
mod runtime_support;
mod superuser;
mod tui;

use crate::external_commands::{ExternalCommandObserver, install_external_event_handlers};
#[cfg(test)]
use crate::external_commands::{llm_usage_text, matches_external_ask_command};
use crate::llm::service::{
    current_active_prompt_profile, current_llm_runtime_config, generate_llm_reply,
    load_current_app_config_doc, load_llm_prompt_store, persist_llm_prompt_store,
    resolve_llm_prompt_store_path,
};
use crate::llm::{LlmPromptPreview, OpenAiResponsesClient, build_prompt_preview};
use crate::runtime_support::{
    EXTERNAL_API_TIMEOUT, ExternalGateway, ExternalGatewaySnapshot, LLM_CONFIG_PATHS,
    LlmCommandRuntime, apply_runtime_log_overrides_from_app_config, describe_runtime_config,
    ensure_default_llm_config_file, ensure_llm_config_file, load_app_config_with_llm_overlay,
    resolve_builtin_plugin_dirs, resolve_llm_config_path, resolve_password_config_path,
};
#[cfg(test)]
use crate::runtime_support::{
    push_explicit_plugin_dir_candidates, push_runtime_plugin_dir_candidates,
};
use app_config::*;
use i18n::{reload_catalog as reload_i18n_catalog, set_current_locale, tr, trf};
use onebot_support::*;
use superuser::SuperuserManager;

const APP_TITLE: &str = "Liteyuki";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;
const DEFAULT_LLM_PROVIDER_BASE_URL: &str = "https://api.openai.com";

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(err) = ensure_default_config_files() {
        eprintln!(
            "{}",
            trf(
                "startup.ensure_default_config_failed",
                &[("err", err.to_string().as_str())],
            )
        );
    }
    if let Err(err) = ensure_default_llm_config_file() {
        eprintln!(
            "{}",
            trf(
                "startup.ensure_default_llm_config_failed",
                &[("err", err.as_str())],
            )
        );
    }

    let settings = match RuntimeSettings::try_load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!(
                "{}",
                trf(
                    "startup.runtime_settings_fallback",
                    &[("err", err.to_string().as_str())],
                )
            );
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
    let locale = resolve_app_locale(&app_config);
    let llm_config = resolve_llm_config(&app_config);
    let disabled_commands = resolve_disabled_scope_commands(&app_config);
    let disabled_plugins = resolve_disabled_plugins(&app_config);
    let llm_runtime = LlmCommandRuntime::new(llm_config.command_prefix.clone());
    let external_gateway = ExternalGateway::new();
    let plugin_dirs = resolve_builtin_plugin_dirs();
    set_current_locale(locale);
    for warning in reload_i18n_catalog(plugin_dirs.iter()) {
        eprintln!("{warning}");
    }
    let superuser_manager =
        match SuperuserManager::load_or_init(resolve_password_config_path().as_path()) {
            Ok(manager) => manager,
            Err(err) => {
                eprintln!(
                    "{}",
                    trf(
                        "startup.password_config_fallback",
                        &[("err", err.to_string().as_str())],
                    )
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

    install_external_event_handlers(
        &bot,
        external_gateway.clone(),
        TuiExternalCommandObserver {
            ui_tx: ui_tx.clone(),
        },
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

#[derive(Clone)]
struct TuiExternalCommandObserver {
    ui_tx: mpsc::UnboundedSender<tui::UiEvent>,
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
        if !whitelist_debug_enabled() {
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

fn reload_from_config(bot: &LiteyukiBot) -> tui::ReloadFuture<'_> {
    Box::pin(async move {
        let (app_config, mut warnings) = load_app_config_with_llm_overlay();
        warnings.extend(collect_runtime_reload_warnings(&app_config));
        let adapters = load_adapter_configs(&app_config)
            .map_err(|err| format!("failed to load adapter configs: {err}"))?;
        let autostart = !adapters.is_empty();
        let locale = resolve_app_locale(&app_config);
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
        set_current_locale(locale);
        warnings.extend(reload_i18n_catalog(bot.plugin_dirs().iter()));
        Ok(tui::ReloadResult {
            adapters,
            adapter_autostart: autostart,
            tui_config,
            locale,
            help_whitelist,
            llm_command_prefix,
            disabled_commands,
            disabled_plugins,
            warnings,
        })
    })
}

fn apply_plugin_policy(
    bot: &LiteyukiBot,
    disabled_plugins: Vec<String>,
) -> tui::PluginPolicyFuture<'_> {
    Box::pin(async move {
        bot.reload_plugins(disabled_plugins)
            .await
            .map_err(|err| format!("failed to apply plugin reload: {err}"))?;
        let warnings = reload_i18n_catalog(bot.plugin_dirs().iter());
        let disabled_count = bot.disabled_plugin_ids().len().to_string();
        let mut message = tr("plugin.reload.success").replace("{count}", disabled_count.as_str());
        if !warnings.is_empty() {
            message.push_str(" | ");
            message.push_str(warnings.join(" | ").as_str());
        }
        Ok(message)
    })
}

fn persist_help_whitelist(entries: Vec<String>) -> Result<String, String> {
    let path = resolve_app_config_path().unwrap_or_else(|| PathBuf::from("config.yaml"));
    write_default_config_if_missing(path.as_path())
        .map_err(|err| format!("failed to ensure config exists: {err}"))?;
    config_edit::persist_onebot_v11_whitelist(path.as_path(), &entries)?;
    let path_display = path.display().to_string();
    let count = entries.len().to_string();
    Ok(trf(
        "whitelist.persist.success",
        &[("path", path_display.as_str()), ("count", count.as_str())],
    )
    .to_string())
}

fn persist_disabled_commands_config(entries: Vec<String>) -> Result<String, String> {
    let path = resolve_app_config_path().unwrap_or_else(|| PathBuf::from("config.yaml"));
    write_default_config_if_missing(path.as_path())
        .map_err(|err| format!("failed to ensure config exists: {err}"))?;
    config_edit::persist_disabled_commands(path.as_path(), &entries)?;
    let path_display = path.display().to_string();
    let count = entries.len().to_string();
    Ok(trf(
        "command_policy.persist.success",
        &[("path", path_display.as_str()), ("count", count.as_str())],
    )
    .to_string())
}

fn persist_disabled_plugins_config(entries: Vec<String>) -> Result<String, String> {
    let path = resolve_app_config_path().unwrap_or_else(|| PathBuf::from("config.yaml"));
    write_default_config_if_missing(path.as_path())
        .map_err(|err| format!("failed to ensure config exists: {err}"))?;
    config_edit::persist_disabled_plugins(path.as_path(), &entries)?;
    let path_display = path.display().to_string();
    let count = entries.len().to_string();
    Ok(trf(
        "plugin_policy.persist.success",
        &[("path", path_display.as_str()), ("count", count.as_str())],
    )
    .to_string())
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
                Ok(trf(
                    "llm.tui.model_updated",
                    &[
                        ("model", model.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::AddApiKeys(new_keys) => {
                let doc = load_current_app_config_doc()?;
                let mut merged = extract_llm_keys_from_doc(&doc);
                merged.extend(new_keys);
                merged = normalize_string_list(merged);
                if merged.is_empty() {
                    return Err(tr("llm.tui.no_valid_api_key"));
                }

                let patch = config_edit::LlmConfigPatch {
                    api_keys: Some(merged.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(trf(
                    "llm.tui.api_keys_updated",
                    &[
                        ("count", merged.len().to_string().as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
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
                    return Ok(trf(
                        "llm.tui.provider_exists",
                        &[("provider_url", provider_url.as_str())],
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
                Ok(trf(
                    "llm.tui.provider_added",
                    &[
                        ("provider_url", provider_url.as_str()),
                        ("count", provider_urls.len().to_string().as_str()),
                        ("active_base_url", active_base_url.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
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
                Ok(trf(
                    "llm.tui.provider_removed",
                    &[
                        ("provider_url", provider_url.as_str()),
                        ("count", provider_urls.len().to_string().as_str()),
                        ("active_base_url", active_base_url.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
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
                Ok(trf(
                    "llm.tui.provider_list",
                    &[
                        ("path", path.display().to_string().as_str()),
                        ("lines", lines.join("\n").as_str()),
                    ],
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
                Ok(trf(
                    "llm.tui.provider_switched",
                    &[
                        ("provider_url", provider_url.as_str()),
                        ("count", provider_urls.len().to_string().as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::SetEnabled { enabled, provider } => {
                let patch = config_edit::LlmConfigPatch {
                    enabled: Some(enabled),
                    provider,
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(trf(
                    "llm.tui.enabled_state",
                    &[
                        (
                            "state",
                            if enabled {
                                tr("llm.tui.state.enabled")
                            } else {
                                tr("llm.tui.state.disabled")
                            }
                            .as_str(),
                        ),
                        ("path", path.display().to_string().as_str()),
                    ],
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

                Ok(trf(
                    "llm.tui.prompt_profiles",
                    &[
                        ("path", path.display().to_string().as_str()),
                        ("lines", lines.as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::PromptUse(name) => {
                let mut store = load_llm_prompt_store()?;
                store.set_active_profile(name.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(trf(
                    "llm.tui.prompt_used",
                    &[
                        ("name", store.active_profile.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::PromptSet { name, soul } => {
                let mut store = load_llm_prompt_store()?;
                store.upsert_profile(name.as_str(), soul.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(trf(
                    "llm.tui.prompt_updated",
                    &[
                        ("name", name.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::PromptRemove(name) => {
                let mut store = load_llm_prompt_store()?;
                store.remove_profile(name.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(trf(
                    "llm.tui.prompt_removed",
                    &[
                        ("name", name.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
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
            Ok(tr("main.llm.empty"))
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

fn resolve_llm_config_write_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_LLM_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    PathBuf::from(LLM_CONFIG_PATHS[0])
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
        return Err(tr("llm.tui.provider_none_configured"));
    }
    if provider_urls.iter().all(|value| value != provider_url) {
        return Err(trf(
            "llm.tui.provider_not_found",
            &[("provider_url", provider_url)],
        ));
    }
    Ok(())
}

async fn probe_llm_configuration(llm_config: &LlmRuntimeConfig) -> Result<String, String> {
    if !llm_config.provider.eq_ignore_ascii_case("openai") {
        return Err(trf(
            "main.llm.provider.unsupported",
            &[("provider", llm_config.provider.as_str())],
        ));
    }
    let Some(api_key) = llm_config.api_keys.first() else {
        return Ok(trf(
            "llm.tui.probe_skipped",
            &[
                ("provider", llm_config.provider.as_str()),
                ("base_url", llm_config.base_url.as_str()),
            ],
        ));
    };

    let client = OpenAiResponsesClient::from_runtime_with_api_key(llm_config, api_key)
        .map_err(|err| err.to_string())?;
    let output = client
        .generate("Reply exactly with: OK")
        .await
        .map_err(|err| err.to_string())?;
    let preview = truncate_text_for_log(output.trim(), 80);
    Ok(trf(
        "llm.tui.probe_success",
        &[
            ("provider", llm_config.provider.as_str()),
            ("model", llm_config.model.as_str()),
            ("output", preview.as_str()),
        ],
    ))
}

fn format_prompt_preview(profile_name: &str, preview: &LlmPromptPreview) -> String {
    let system_prompt = if preview.system_prompt.trim().is_empty() {
        tr("llm.tui.empty_value")
    } else {
        preview.system_prompt.clone()
    };
    let composed_user_prompt = if preview.composed_user_prompt.trim().is_empty() {
        tr("llm.tui.empty_value")
    } else {
        preview.composed_user_prompt.clone()
    };

    trf(
        "llm.tui.prompt_preview",
        &[
            ("profile", profile_name),
            ("system_prompt", system_prompt.as_str()),
            ("composed_user_prompt", composed_user_prompt.as_str()),
            ("combined_prompt", preview.combined_prompt.as_str()),
        ],
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

    fn lock_llm_config_env() -> std::sync::MutexGuard<'static, ()> {
        llm_config_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        let _lock = lock_llm_config_env();
        let path = temp_llm_config_path("provider-use-invalid");
        let source = "llm:\n  base_url: https://api.openai.com\n  provider_urls:\n    - https://api.openai.com\n    - https://tokenflux.dev/v1\n";
        fs::write(&path, source).expect("test llm config should be written");
        let _env_guard = EnvVarGuard::set("LY_LLM_CONFIG_PATH", path.as_path());

        let result = run_llm_command_for_test(tui::LlmCommandRequest::UseProviderUrl(
            "https://typo.example/v1".to_string(),
        ))
        .expect_err("unregistered provider url should be rejected");

        assert!(result.contains("https://typo.example/v1"));
        let updated = fs::read_to_string(&path).expect("test llm config should remain readable");
        assert_eq!(updated, source);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn llm_provider_use_switches_to_registered_base_url() {
        let _lock = lock_llm_config_env();
        let path = temp_llm_config_path("provider-use-valid");
        let source = "llm:\n  base_url: https://api.openai.com\n  provider_urls:\n    - https://api.openai.com\n    - https://tokenflux.dev/v1\n";
        fs::write(&path, source).expect("test llm config should be written");
        let _env_guard = EnvVarGuard::set("LY_LLM_CONFIG_PATH", path.as_path());

        let result = run_llm_command_for_test(tui::LlmCommandRequest::UseProviderUrl(
            "https://tokenflux.dev/v1".to_string(),
        ))
        .expect("registered provider url should switch successfully");

        assert!(result.contains("https://tokenflux.dev/v1"));
        assert!(result.contains("count=2"));
        assert!(result.contains(path.display().to_string().as_str()));
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

    #[test]
    fn builtin_plugin_candidates_cover_runtime_and_dev_layouts() {
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
}
