use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use liteyukibot_core::adapter::AdapterManager;
use liteyukibot_core::{AdapterPacket, LiteyukiBot, PluginSdk, Rule, SessionEvent};
use serde_json::Value;

use crate::command_registry::{
    AdapterProtocol, BuiltinCommandId, CommandNameOverrides, CommandScope,
    command_argument_for_message, matches_builtin_command_message,
};
use crate::i18n::{tr, trf};
use crate::llm::service::generate_llm_reply;
use crate::onebot_support::{
    build_onebot_v11_text_reply_payload, is_help_command, is_help_session_allowed,
    is_onebot_private_message, is_onebot_v11_payload, matched_help_whitelist_entry,
    parse_su_password_argument, render_external_help_text_with_plugins, value_to_string,
};
use crate::runtime_support::{ExternalGateway, ExternalGatewaySnapshot, LlmCommandRuntime};
use crate::superuser::SuperuserManager;

pub(crate) trait ExternalCommandObserver: Clone + Send + Sync + 'static {
    fn record_stats(&self, snapshot: &ExternalGatewaySnapshot);

    fn on_help_whitelist_evaluated(
        &self,
        _event: &SessionEvent,
        _matched_entry: Option<&str>,
        _whitelist_size: usize,
        _allowed: bool,
    ) {
    }
}

pub(crate) fn install_external_event_handlers<O: ExternalCommandObserver>(
    bot: &LiteyukiBot,
    gateway: ExternalGateway,
    observer: O,
    help_whitelist: Arc<RwLock<HashSet<String>>>,
    llm_runtime: LlmCommandRuntime,
    plugin_sdk: PluginSdk,
    superuser_manager: SuperuserManager,
) {
    let adapter_manager = bot.adapter_manager().clone();
    let gateway_for_su = gateway.clone();
    let observer_for_su = observer.clone();
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
            let observer = observer_for_su.clone();
            let superuser_manager = superuser_for_su.clone();
            let plugin_sdk = plugin_sdk_for_su.clone();
            async move {
                handle_external_su_command(
                    &adapter_manager,
                    &gateway,
                    &observer,
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
    let observer_for_help = observer.clone();
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
            let observer = observer_for_help.clone();
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
                        &observer,
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
                        &observer,
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
                observer.on_help_whitelist_evaluated(
                    event.as_ref(),
                    matched_entry.as_deref(),
                    whitelist_size,
                    allowed,
                );
                if !allowed {
                    return Ok(());
                }
                reply_help_command(
                    &adapter_manager,
                    &gateway,
                    &observer,
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
    let observer_for_ask = observer.clone();
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
            let observer = observer_for_ask.clone();
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
                        &observer,
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
                        &observer,
                        event.as_ref(),
                        tr("main.auth.su_required").as_str(),
                        "su-required-ask",
                    )
                    .await;
                }
                reply_ask_command(&adapter_manager, &gateway, &observer, &llm_runtime, event).await
            }
        },
    );
}

pub(crate) fn matches_external_ask_command(message: &str, llm_runtime: &LlmCommandRuntime) -> bool {
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

pub(crate) fn llm_usage_text(command_prefix: &str) -> String {
    trf("main.ask.usage", &[("command", command_prefix)])
}

fn command_disabled_text(command_name: &str) -> String {
    trf("main.command.disabled", &[("command", command_name)])
}

async fn handle_external_su_command<O: ExternalCommandObserver>(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    observer: &O,
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
            observer,
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
            observer,
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
            observer,
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
            observer,
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
        observer,
        event.as_ref(),
        text.as_str(),
        "su-granted",
    )
    .await
}

async fn reply_help_command<O: ExternalCommandObserver>(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    observer: &O,
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
        observer,
        event.as_ref(),
        help_text.as_str(),
        "liteyuki-help",
    )
    .await
}

async fn reply_ask_command<O: ExternalCommandObserver>(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    observer: &O,
    llm_runtime: &LlmCommandRuntime,
    event: Arc<SessionEvent>,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    observer.record_stats(&gateway.record_command_hit());

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
        observer,
        event.as_ref(),
        format!("ask-{}", event.event_id),
        echo,
        payload,
    )
    .await
}

async fn reply_external_text<O: ExternalCommandObserver>(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    observer: &O,
    event: &SessionEvent,
    text: &str,
    echo_prefix: &str,
) -> Result<(), String> {
    if !is_onebot_v11_payload(&event.payload) {
        return Ok(());
    }
    observer.record_stats(&gateway.record_command_hit());
    let echo = gateway.next_echo(echo_prefix);
    let payload = build_onebot_v11_text_reply_payload(event, &echo, text)
        .ok_or_else(|| "failed to build onebot v11 text response".to_string())?;
    dispatch_onebot_reply(
        adapter_manager,
        gateway,
        observer,
        event,
        format!("{echo_prefix}-{}", event.event_id),
        echo,
        payload,
    )
    .await
}

async fn dispatch_onebot_reply<O: ExternalCommandObserver>(
    adapter_manager: &AdapterManager,
    gateway: &ExternalGateway,
    observer: &O,
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

    observer.record_stats(&gateway.track_request(echo.clone()));
    let packet = AdapterPacket::new(packet_id, "onebot.v11.api.send_msg", payload);
    let send_result = adapter_manager.send(&adapter_id, packet).await;
    if let Err(err) = send_result {
        observer.record_stats(&gateway.mark_send_failed(&echo));
        return Err(format!("send reply failed: {err}"));
    }
    Ok(())
}
