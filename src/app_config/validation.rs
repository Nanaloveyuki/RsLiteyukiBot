use super::access::{
    config_adapters, config_commands, config_connect, config_i18n, config_llm, config_onebot_v11,
    config_plugins, config_tui_resume, normalize_disabled_command_entry, normalize_plugin_id_entry,
};
use super::*;
use crate::hardcode_data::llm::DEFAULT_LLM_PROVIDER;
use crate::utils::llm_config::normalize_provider_url;

fn validate_adapter_section(doc: &AppConfigDoc, locale: AppLocale, warnings: &mut Vec<String>) {
    let Some(adapters) = config_adapters(doc) else {
        return;
    };

    let mut seen = HashSet::new();
    for adapter in adapters {
        if let Err(err) = adapter.validate() {
            warnings.push(trf_for(
                locale,
                "config.warn.invalid_adapter",
                &[("adapter", adapter.id.as_str()), ("err", err.as_str())],
            ));
        }
        if !seen.insert(adapter.id.clone()) {
            warnings.push(trf_for(
                locale,
                "config.warn.duplicate_adapter",
                &[("adapter", adapter.id.as_str())],
            ));
        }
    }
}

fn validate_tui_section(doc: &AppConfigDoc, locale: AppLocale, warnings: &mut Vec<String>) {
    let Some(resume) = config_tui_resume(doc) else {
        return;
    };

    if let Some(path) = resume.store_path.as_deref()
        && path.trim().is_empty()
    {
        super::push_should_not_be_empty_warning(warnings, locale, "tui.resume.store_path");
    }
    if resume.max_sessions.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(warnings, locale, "tui.resume.max_sessions");
    }
    if resume.max_size_mib.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(warnings, locale, "tui.resume.max_size_mib");
    }
}

fn validate_i18n_section(doc: &AppConfigDoc, warnings: &mut Vec<String>) {
    if let Some(i18n) = config_i18n(doc)
        && let Some(locale) = i18n.locale.as_deref()
        && AppLocale::parse(locale).is_none()
    {
        warnings.push(trf_for(
            super::resolve_app_locale(doc),
            "config.warn.locale_supported",
            &[("field", "i18n.locale"), ("choices", "zh-CN|en-US")],
        ));
    }
}

fn validate_connect_section(doc: &AppConfigDoc, locale: AppLocale, warnings: &mut Vec<String>) {
    let Some(connect) = config_connect(doc) else {
        return;
    };

    if let Some(ws) = &connect.websocket {
        validate_websocket_connect(doc, locale, warnings, ws);
    }
    if let Some(http) = &connect.tcp_http {
        validate_http_connect(locale, warnings, http);
    }
    if let Some(sse) = &connect.sse {
        validate_sse_connect(locale, warnings, sse);
    }
}

fn validate_websocket_connect(
    doc: &AppConfigDoc,
    locale: AppLocale,
    warnings: &mut Vec<String>,
    ws: &WebSocketConnectSection,
) {
    if ws.max_payload_size.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(
            warnings,
            locale,
            "connect.websocket.max_payload_size",
        );
    }
    if ws.max_connections.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(
            warnings,
            locale,
            "connect.websocket.max_connections",
        );
    }
    if super::has_empty_item(ws.urls.as_ref()) {
        super::push_empty_values_warning(warnings, locale, "connect.websocket.urls");
    }
    if ws.enabled.unwrap_or(false) {
        let has_nested = ws.forward.is_some() || ws.reverse.is_some();
        if !has_nested
            && ws.url.is_none()
            && !super::has_non_empty_list(ws.urls.as_ref())
            && ws.port.is_none()
        {
            warnings.push(super::localized_doc_text(
                doc,
                "config.warn.websocket_missing_url_or_port",
            ));
        }
    }
    if let Some(forward) = &ws.forward {
        validate_websocket_forward(doc, locale, warnings, ws, forward);
    }
    if let Some(reverse) = &ws.reverse {
        validate_websocket_reverse(doc, locale, warnings, ws, reverse);
    }
}

fn validate_websocket_forward(
    doc: &AppConfigDoc,
    locale: AppLocale,
    warnings: &mut Vec<String>,
    ws: &WebSocketConnectSection,
    forward: &WebSocketEndpointSection,
) {
    if super::has_empty_item(forward.urls.as_ref()) {
        super::push_empty_values_warning(warnings, locale, "connect.websocket.forward.urls");
    }

    let host = forward.host.as_deref().or(ws.host.as_deref());
    let port = forward.port.or(ws.port);
    if forward.enabled.unwrap_or(false)
        && !super::websocket_has_multi_urls(forward, ws)
        && forward.url.is_none()
        && ws.url.is_none()
        && (port.is_none() || host.is_none())
    {
        warnings.push(super::localized_doc_text(
            doc,
            "config.warn.websocket_forward_incomplete",
        ));
    }

    if forward.max_payload_size.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(
            warnings,
            locale,
            "connect.websocket.forward.max_payload_size",
        );
    }
    if forward.max_connections.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(
            warnings,
            locale,
            "connect.websocket.forward.max_connections",
        );
    }
}

fn validate_websocket_reverse(
    doc: &AppConfigDoc,
    locale: AppLocale,
    warnings: &mut Vec<String>,
    ws: &WebSocketConnectSection,
    reverse: &WebSocketEndpointSection,
) {
    if super::has_empty_item(reverse.urls.as_ref()) {
        super::push_empty_values_warning(warnings, locale, "connect.websocket.reverse.urls");
    }

    let port = reverse.port.or(ws.port);
    if reverse.enabled.unwrap_or(false)
        && !super::websocket_has_multi_urls(reverse, ws)
        && reverse.url.is_none()
        && ws.url.is_none()
        && port.is_none()
    {
        warnings.push(super::localized_doc_text(
            doc,
            "config.warn.websocket_reverse_incomplete",
        ));
    }

    if reverse.max_payload_size.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(
            warnings,
            locale,
            "connect.websocket.reverse.max_payload_size",
        );
    }
    if reverse.max_connections.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(
            warnings,
            locale,
            "connect.websocket.reverse.max_connections",
        );
    }
}

fn validate_http_connect(locale: AppLocale, warnings: &mut Vec<String>, http: &HttpConnectSection) {
    if http.max_payload_size.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(
            warnings,
            locale,
            "connect.tcp-http.max_payload_size",
        );
    }
    if http.max_connections.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(
            warnings,
            locale,
            "connect.tcp-http.max_connections",
        );
    }
    if super::has_empty_item(http.urls.as_ref()) {
        super::push_empty_values_warning(warnings, locale, "connect.tcp-http.urls");
    }
}

fn validate_sse_connect(locale: AppLocale, warnings: &mut Vec<String>, sse: &SseConnectSection) {
    if sse.max_payload_size.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(warnings, locale, "connect.sse.max_payload_size");
    }
    if sse.max_connections.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(warnings, locale, "connect.sse.max_connections");
    }
    if super::has_empty_item(sse.urls.as_ref()) {
        super::push_empty_values_warning(warnings, locale, "connect.sse.urls");
    }
}

fn validate_llm_section(doc: &AppConfigDoc, locale: AppLocale, warnings: &mut Vec<String>) {
    let Some(llm) = config_llm(doc) else {
        return;
    };

    if llm.timeout_seconds.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(warnings, locale, "llm.timeout_seconds");
    }
    if llm
        .temperature
        .is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
    {
        super::push_invalid_range_warning(warnings, locale, "llm.temperature", "0..=2");
    }
    if llm
        .top_p
        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        super::push_invalid_range_warning(warnings, locale, "llm.top_p", "0..=1");
    }
    if llm.top_k.is_some_and(|value| value == 0) {
        super::push_should_be_positive_warning(warnings, locale, "llm.top_k");
    }
    if llm
        .frequency_penalty
        .is_some_and(|value| !value.is_finite() || !(-2.0..=2.0).contains(&value))
    {
        super::push_invalid_range_warning(warnings, locale, "llm.frequency_penalty", "-2..=2");
    }
    if llm
        .presence_penalty
        .is_some_and(|value| !value.is_finite() || !(-2.0..=2.0).contains(&value))
    {
        super::push_invalid_range_warning(warnings, locale, "llm.presence_penalty", "-2..=2");
    }
    if llm
        .base_url
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        warnings.push(trf_for(
            locale,
            "config.warn.deprecated_move",
            &[("field", "llm.base_url"), ("target", "llm-config.yaml")],
        ));
    }
    if llm
        .api_keys
        .as_ref()
        .is_some_and(|keys| keys.iter().any(|key| key.trim().is_empty()))
    {
        super::push_empty_values_warning(warnings, locale, "llm.api_keys");
    }
    if llm
        .provider_urls
        .as_ref()
        .is_some_and(|urls| urls.iter().any(|url| normalize_provider_url(url).is_none()))
    {
        super::push_empty_values_warning(warnings, locale, "llm.provider_urls");
    }
    if llm.enabled.unwrap_or(false) {
        validate_enabled_llm_section(doc, warnings, llm);
    }
    if let Some(provider) = llm.provider.as_deref()
        && !provider.trim().is_empty()
        && !provider.eq_ignore_ascii_case(DEFAULT_LLM_PROVIDER)
    {
        warnings.push(trf_for(
            locale,
            "config.warn.llm_provider_unsupported",
            &[
                ("provider", provider.trim()),
                ("builtin", DEFAULT_LLM_PROVIDER),
            ],
        ));
    }
    if llm
        .command_prefix
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        super::push_should_not_be_empty_warning(warnings, locale, "llm.command_prefix");
    }
}

fn validate_enabled_llm_section(
    doc: &AppConfigDoc,
    warnings: &mut Vec<String>,
    llm: &LlmConfigSection,
) {
    let has_non_empty_api_key = llm
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_non_empty_api_keys = llm
        .api_keys
        .as_ref()
        .is_some_and(|keys| keys.iter().any(|value| !value.trim().is_empty()));
    if !has_non_empty_api_key && !has_non_empty_api_keys {
        warnings.push(super::localized_doc_text(
            doc,
            "config.warn.llm_api_key_missing",
        ));
    }
    if llm
        .model
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        warnings.push(super::localized_doc_text(
            doc,
            "config.warn.llm_model_missing",
        ));
    }
}

fn validate_onebot_section(doc: &AppConfigDoc, locale: AppLocale, warnings: &mut Vec<String>) {
    let Some(onebot) = config_onebot_v11(doc) else {
        return;
    };

    for entry in &onebot.whitelist {
        if entry.as_token().is_empty() {
            super::push_empty_values_warning(warnings, locale, "onebot-v11.whitelist");
            break;
        }
    }
}

fn validate_commands_section(doc: &AppConfigDoc, warnings: &mut Vec<String>) {
    let Some(commands) = config_commands(doc) else {
        return;
    };

    for entry in &commands.disabled {
        if normalize_disabled_command_entry(entry).is_none() {
            warnings.push(super::localized_doc_text(
                doc,
                "config.warn.commands_disabled_format",
            ));
            break;
        }
    }
}

fn validate_plugins_section(doc: &AppConfigDoc, warnings: &mut Vec<String>) {
    let Some(plugins) = config_plugins(doc) else {
        return;
    };

    for entry in &plugins.disabled {
        if normalize_plugin_id_entry(entry).is_none() {
            warnings.push(super::localized_doc_text(
                doc,
                "config.warn.plugins_disabled_format",
            ));
            break;
        }
    }
}

pub(crate) fn validate_app_config(doc: &AppConfigDoc) -> Vec<String> {
    let locale = super::resolve_app_locale(doc);
    let mut warnings = Vec::new();
    validate_adapter_section(doc, locale, &mut warnings);
    validate_tui_section(doc, locale, &mut warnings);
    validate_i18n_section(doc, &mut warnings);
    validate_connect_section(doc, locale, &mut warnings);
    validate_llm_section(doc, locale, &mut warnings);
    validate_onebot_section(doc, locale, &mut warnings);
    validate_commands_section(doc, &mut warnings);
    validate_plugins_section(doc, &mut warnings);
    warnings
}

pub(crate) fn prime_reload_warning_state(doc: &AppConfigDoc) {
    let mut lock = LAST_RELOAD_WARNING_STATE
        .lock()
        .expect("reload warning state lock should not be poisoned");
    *lock = Some(ReloadWarningState::from_doc(doc));
}

pub(crate) fn collect_runtime_reload_warnings(doc: &AppConfigDoc) -> Vec<String> {
    let current = ReloadWarningState::from_doc(doc);
    let mut lock = LAST_RELOAD_WARNING_STATE
        .lock()
        .expect("reload warning state lock should not be poisoned");
    let warnings = runtime_reload_warnings(lock.as_ref(), &current);
    *lock = Some(current);
    warnings
}

pub(crate) fn runtime_reload_warnings(
    previous: Option<&ReloadWarningState>,
    current: &ReloadWarningState,
) -> Vec<String> {
    let mut warnings = Vec::new();

    let runtime_changed = previous.is_none_or(|prev| prev.runtime != current.runtime);
    let runtime_sensitive = previous
        .is_some_and(|prev| runtime_has_hot_reload_sensitive_fields(&prev.runtime))
        || runtime_has_hot_reload_sensitive_fields(&current.runtime);
    if runtime_changed && runtime_sensitive {
        warnings.push(super::tr("config.warn.reload.runtime_sensitive"));
    }

    let log_changed = previous.is_none_or(|prev| prev.log != current.log);
    let log_sensitive = previous.is_some_and(|prev| log_has_startup_only_fields(&prev.log))
        || log_has_startup_only_fields(&current.log);
    if log_changed && log_sensitive {
        warnings.push(super::tr("reload.warn.log_restart_recommended"));
    }

    warnings
}

pub(crate) fn runtime_has_hot_reload_sensitive_fields(
    runtime: &Option<RuntimeConfigSection>,
) -> bool {
    runtime.as_ref().is_some_and(|runtime| {
        runtime.worker_count.is_some()
            || runtime.ingress_queue.is_some()
            || runtime.worker_queue.is_some()
    })
}

pub(crate) fn log_has_startup_only_fields(log: &Option<LogConfigSection>) -> bool {
    log.as_ref().is_some_and(|log| {
        log.mode.is_some()
            || log.level.is_some()
            || log.timezone.is_some()
            || log.timestamp_format.is_some()
            || log.timestamp_pattern.is_some()
    })
}
