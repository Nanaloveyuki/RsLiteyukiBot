use serde_json::{Value, json};

use super::{
    WebLlmChatRequest, effective_messages, execute_provider_chat, normalize_base_url,
    parse_json_body,
};

use crate::llm::service::{
    current_active_prompt_profile, current_llm_runtime_config, resolve_provider_id,
};
use crate::runtime_support::next_llm_api_key_index;
use crate::web::host::{WebHostService, run_async_for_web_host};
use crate::{LogLevel, emit_console_log};

pub(super) fn llm_chat_payload(service: &WebHostService, request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let chat_request: WebLlmChatRequest = serde_json::from_value(request_body).map_err(|err| {
        let message = format!("invalid LLM chat payload: {err}");
        emit_console_log(
            LogLevel::Warn,
            "web.llm",
            format!("frontend chat rejected: {message}"),
        );
        message
    })?;

    let llm_config = current_llm_runtime_config().map_err(|err| {
        emit_console_log(
            LogLevel::Error,
            "web.llm",
            format!("failed to load runtime LLM config for frontend chat: {err}"),
        );
        err
    })?;
    if !llm_config.enabled {
        emit_console_log(
            LogLevel::Warn,
            "web.llm",
            "frontend chat rejected because LLM is disabled",
        );
        return Err("LLM is disabled".to_string());
    }
    let prompt_profile = current_active_prompt_profile().map_err(|err| {
        emit_console_log(
            LogLevel::Error,
            "web.llm",
            format!("failed to load active prompt profile for frontend chat: {err}"),
        );
        err
    })?;
    let api_key = pick_llm_api_key(&llm_config).map_err(|err| {
        emit_console_log(
            LogLevel::Warn,
            "web.llm",
            format!("frontend chat rejected because no API key is configured: {err}"),
        );
        err
    })?;

    let effective_base_url = chat_request
        .base_url
        .as_deref()
        .map(normalize_base_url)
        .unwrap_or_else(|| normalize_base_url(llm_config.base_url.as_str()));
    let provider_id = resolve_provider_id(
        Some(llm_config.provider.as_str()),
        Some(effective_base_url.as_str()),
    );
    let effective_model = chat_request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(llm_config.model.as_str())
        .to_string();
    let reasoning_effort = chat_request
        .reasoning_effort
        .as_deref()
        .and_then(normalize_reasoning_effort_input)
        .map(ToString::to_string);
    let request_summary = summarize_frontend_chat_request(
        &chat_request,
        provider_id.as_str(),
        effective_base_url.as_str(),
        effective_model.as_str(),
        prompt_profile.name.as_str(),
        reasoning_effort.as_deref(),
    );

    validate_sampling_args(
        chat_request.temperature,
        chat_request.top_p,
        chat_request.top_k,
        chat_request.frequency_penalty,
        chat_request.presence_penalty,
    )
    .map_err(|err| {
        emit_console_log(
            LogLevel::Warn,
            "web.llm",
            format!("frontend chat rejected ({request_summary}): {err}"),
        );
        err
    })?;

    emit_console_log(
        LogLevel::Info,
        "web.llm",
        format!("frontend chat started ({request_summary})"),
    );

    let execution = run_async_for_web_host(execute_provider_chat(
        service,
        &chat_request,
        &llm_config,
        &api_key,
        prompt_profile.soul.as_str(),
        provider_id.as_str(),
        effective_base_url.as_str(),
        effective_model.as_str(),
        reasoning_effort.as_deref(),
    ))
    .map_err(|err| {
        emit_console_log(
            LogLevel::Error,
            "web.llm",
            format!("frontend chat failed ({request_summary}): {err}"),
        );
        err
    })?;

    emit_console_log(
        LogLevel::Info,
        "web.llm",
        format!(
            "frontend chat completed ({request_summary}, output_chars={})",
            execution.message.chars().count()
        ),
    );

    Ok(json!({
        "message": execution.message,
        "model": execution.model,
        "baseUrl": execution.base_url,
        "promptProfile": prompt_profile.name,
    }))
}

pub(super) fn summarize_frontend_chat_request(
    request: &WebLlmChatRequest,
    provider_id: &str,
    effective_base_url: &str,
    effective_model: &str,
    prompt_profile: &str,
    reasoning_effort: Option<&str>,
) -> String {
    let messages = effective_messages(request);
    let attachment_count = messages
        .iter()
        .map(|message| message.attachments.len())
        .sum::<usize>();
    let input_chars = messages
        .iter()
        .map(|message| message.content.trim().chars().count())
        .sum::<usize>();
    let mut parts = vec![
        format!("provider={provider_id}"),
        format!("base_url={effective_base_url}"),
        format!("model={effective_model}"),
        format!("prompt_profile={prompt_profile}"),
        format!("turns={}", messages.len()),
        format!("attachments={attachment_count}"),
        format!("input_chars={input_chars}"),
    ];

    if let Some(reasoning_effort) = reasoning_effort {
        parts.push(format!("reasoning={reasoning_effort}"));
    }

    parts.join(", ")
}

pub(super) fn pick_llm_api_key(
    llm_config: &crate::app_config::LlmRuntimeConfig,
) -> Result<String, String> {
    let index = next_llm_api_key_index(llm_config.api_keys.len())
        .ok_or_else(|| "no api key configured".to_string())?;
    llm_config
        .api_keys
        .get(index)
        .cloned()
        .ok_or_else(|| "no api key configured".to_string())
}

pub(super) fn validate_sampling_args(
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    frequency_penalty: Option<f32>,
    presence_penalty: Option<f32>,
) -> Result<(), String> {
    if temperature.is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value)) {
        return Err("temperature should be within 0..=2".to_string());
    }
    if top_p.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        return Err("topP should be within 0..=1".to_string());
    }
    if top_k.is_some_and(|value| value == 0) {
        return Err("topK should be > 0".to_string());
    }
    if frequency_penalty.is_some_and(|value| !value.is_finite() || !(-2.0..=2.0).contains(&value)) {
        return Err("frequencyPenalty should be within -2..=2".to_string());
    }
    if presence_penalty.is_some_and(|value| !value.is_finite() || !(-2.0..=2.0).contains(&value)) {
        return Err("presencePenalty should be within -2..=2".to_string());
    }
    Ok(())
}

pub(super) fn normalize_reasoning_effort_input(raw: &str) -> Option<&str> {
    let value = raw.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(value)
    }
}
