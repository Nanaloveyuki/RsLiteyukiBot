use std::time::Instant;

use serde_json::{Value, json};

use crate::app_config::LlmManagedProviderConfig;
use crate::llm::service::{
    LlmProviderApiFamily, current_active_prompt_profile, provider_api_family, resolve_provider_id,
};
use crate::web::host::run_async_for_web_host;

use super::super::provider_catalog::{
    detect_provider_id, model_options_for_provider, normalize_base_url,
};
use super::super::request_builders::{
    build_anthropic_request, build_chat_completions_request_payload, build_gemini_request,
    build_openai_compatible_request_payload,
};
use super::super::transport::{
    collect_runtime_headers, join_api_endpoint, send_json_get_request,
    send_json_request_with_optional_auth,
};
use super::super::types::{
    PreparedProviderRequest, ProviderAuth, WebLlmChatRequest, WebLlmMessage, WebLlmRuntimeConfig,
};

const DEFAULT_ANTHROPIC_API_VERSION: &str = "2023-06-01";

pub(in super::super) fn discover_models_for_provider(
    provider: &LlmManagedProviderConfig,
) -> Result<(Vec<String>, &'static str), String> {
    let provider_id = provider
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| provider.base_url.as_deref().map(detect_provider_id))
        .unwrap_or_else(|| "openai-compatible".to_string());

    match provider_id.as_str() {
        "openai" | "openai-compatible" | "openrouter" | "kimi" | "qwen" => {
            match run_async_for_web_host(fetch_openai_style_models(provider, provider_id.as_str()))
            {
                Ok(models) => Ok((models, "remote")),
                Err(_) => Ok((model_options_for_provider(provider_id.as_str()), "catalog")),
            }
        }
        _ => Ok((model_options_for_provider(provider_id.as_str()), "catalog")),
    }
}

pub(in super::super) fn probe_single_provider_model(
    provider: &LlmManagedProviderConfig,
    model_id: &str,
) -> Value {
    let started_at = Instant::now();
    let prepared = prepare_provider_request(provider, model_id);
    match prepared {
        Ok(prepared) => {
            let result = run_async_for_web_host(execute_prepared_provider_request(
                provider_timeout_ms(provider),
                &prepared,
            ));
            match result {
                Ok(_) => json!({
                    "modelId": model_id,
                    "ok": true,
                    "latencyMs": started_at.elapsed().as_millis() as u64,
                }),
                Err(err) => json!({
                    "modelId": model_id,
                    "ok": false,
                    "latencyMs": started_at.elapsed().as_millis() as u64,
                    "error": err,
                }),
            }
        }
        Err(err) => json!({
            "modelId": model_id,
            "ok": false,
            "latencyMs": started_at.elapsed().as_millis() as u64,
            "error": err,
        }),
    }
}

pub(in super::super) fn prepare_provider_request(
    provider: &LlmManagedProviderConfig,
    model_id: &str,
) -> Result<PreparedProviderRequest, String> {
    let provider_id =
        resolve_provider_id(provider.provider.as_deref(), provider.base_url.as_deref());
    let base_url = provider
        .base_url
        .as_deref()
        .map(normalize_base_url)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "provider base_url is empty".to_string())?;
    let prompt_profile = current_active_prompt_profile()?;
    let chat_request = build_probe_chat_request();
    let runtime = WebLlmRuntimeConfig {
        base_url: base_url.clone(),
        model: model_id.to_string(),
        timeout_ms: provider_timeout_ms(provider),
        system_prompt: None,
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
        parallel_tool_calls: false,
        reasoning_effort: None,
        default_headers: provider.headers.clone().unwrap_or_default(),
    };

    match provider_api_family(provider_id.as_str()) {
        LlmProviderApiFamily::ChatCompletions => Ok(PreparedProviderRequest {
            endpoint: join_api_endpoint(base_url.as_str(), "chat/completions"),
            auth: Some(ProviderAuth::Bearer),
            api_key: provider.api_key.clone(),
            extra_headers: collect_runtime_headers(&runtime.default_headers, &[]),
            payload: build_chat_completions_request_payload(
                &chat_request,
                None,
                prompt_profile.soul.as_str(),
                &runtime,
            )?,
        }),
        LlmProviderApiFamily::AnthropicMessages => Ok(PreparedProviderRequest {
            endpoint: join_api_endpoint(base_url.as_str(), "v1/messages"),
            auth: Some(ProviderAuth::ApiKeyHeader("x-api-key".to_string())),
            api_key: provider.api_key.clone(),
            extra_headers: collect_runtime_headers(
                &runtime.default_headers,
                &[("anthropic-version", DEFAULT_ANTHROPIC_API_VERSION)],
            ),
            payload: build_anthropic_request(
                &chat_request,
                None,
                prompt_profile.soul.as_str(),
                model_id,
                None,
                None,
                None,
                None,
            )?,
        }),
        LlmProviderApiFamily::GeminiGenerateContent => Ok(PreparedProviderRequest {
            endpoint: join_api_endpoint(
                base_url.as_str(),
                format!("models/{model_id}:generateContent").as_str(),
            ),
            auth: Some(ProviderAuth::ApiKeyHeader("x-goog-api-key".to_string())),
            api_key: provider.api_key.clone(),
            extra_headers: collect_runtime_headers(&runtime.default_headers, &[]),
            payload: build_gemini_request(
                &chat_request,
                None,
                prompt_profile.soul.as_str(),
                model_id,
                None,
                None,
                None,
                None,
            )?,
        }),
        LlmProviderApiFamily::Responses => Ok(PreparedProviderRequest {
            endpoint: join_api_endpoint(base_url.as_str(), "responses"),
            auth: Some(ProviderAuth::Bearer),
            api_key: provider.api_key.clone(),
            extra_headers: collect_runtime_headers(&runtime.default_headers, &[]),
            payload: build_openai_compatible_request_payload(
                &chat_request,
                &runtime,
                prompt_profile.soul.as_str(),
                provider_id.as_str(),
            )?,
        }),
    }
}

async fn fetch_openai_style_models(
    provider: &LlmManagedProviderConfig,
    provider_id: &str,
) -> Result<Vec<String>, String> {
    let base_url = provider
        .base_url
        .as_deref()
        .map(normalize_base_url)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "provider base_url is empty".to_string())?;
    let endpoint = join_api_endpoint(base_url.as_str(), "models");
    let auth = if provider_id == "openrouter" && provider.api_key.as_deref().is_none() {
        None
    } else {
        Some(ProviderAuth::Bearer)
    };
    let runtime_headers = provider.headers.clone().unwrap_or_default();
    let response = send_json_get_request(
        provider_timeout_ms(provider),
        endpoint.as_str(),
        provider.api_key.as_deref(),
        auth,
        &collect_runtime_headers(&runtime_headers, &[]),
    )
    .await?;
    let models = response
        .get("data")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Err("provider returned an empty model list".to_string());
    }
    Ok(models)
}

fn provider_timeout_ms(provider: &LlmManagedProviderConfig) -> u64 {
    provider
        .timeout_seconds
        .filter(|value| *value > 0)
        .unwrap_or(120)
        .saturating_mul(1000)
}

fn build_probe_chat_request() -> WebLlmChatRequest {
    WebLlmChatRequest {
        message: "reply with pong only".to_string(),
        messages: vec![WebLlmMessage {
            role: "user".to_string(),
            content: "reply with pong only".to_string(),
            attachments: Vec::new(),
        }],
        attachments: Vec::new(),
        base_url: None,
        model: None,
        reasoning_effort: None,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    }
}

async fn execute_prepared_provider_request(
    timeout_ms: u64,
    prepared: &PreparedProviderRequest,
) -> Result<Value, String> {
    send_json_request_with_optional_auth(
        timeout_ms,
        prepared.endpoint.as_str(),
        prepared.api_key.as_deref(),
        prepared.auth.clone(),
        prepared.extra_headers.as_slice(),
        &prepared.payload,
    )
    .await
}
