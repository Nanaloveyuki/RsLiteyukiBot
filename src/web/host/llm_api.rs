use super::*;

use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::app_config::{LlmManagedModelConfig, LlmManagedProviderConfig};
use crate::config_edit::LlmConfigPatch;
use crate::config_paths::resolve_default_llm_config_path;
use crate::llm::client::extract_output_text;
use crate::llm::service::{
    LlmProviderApiFamily, current_active_prompt_profile, current_llm_runtime_config,
    detect_provider_id_from_base_url, load_current_app_config_doc, load_llm_prompt_store,
    persist_llm_prompt_store, provider_api_family, resolve_llm_prompt_store_path,
    resolve_provider_id,
};
use crate::llm::tools::{ToolManager, merge_system_prompt_sections};
use crate::llm::{
    LlmClientError, LlmPromptPreview, LlmPromptProfile, LlmPromptStore, OpenAiResponsesClient,
    OpenAiRuntimeConfig, build_prompt_preview,
};
use crate::runtime_support::{
    ensure_llm_config_file, next_llm_api_key_index, resolve_llm_config_path,
};

const DEFAULT_WEB_LLM_MAX_OUTPUT_TOKENS: u32 = 2048;
const DEFAULT_ANTHROPIC_API_VERSION: &str = "2023-06-01";

pub(super) fn route_llm_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/LLM/GetSettings" {
        let body = match llm_settings_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/Chat" {
        if !method.eq_ignore_ascii_case("POST") {
            let body = napcat_err(-1, "LLM/Chat only accepts POST");
            return Some(napcat_response(body, is_head));
        }

        let body = match llm_chat_payload(service, request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/GetManagerState" {
        let body = match llm_manager_state_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/SaveManagerState" {
        if let Some(response) = reject_non_post_method(method, "LLM/SaveManagerState", is_head) {
            return Some(response);
        }

        let body = match save_llm_manager_state(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/FetchModels" {
        if let Some(response) = reject_non_post_method(method, "LLM/FetchModels", is_head) {
            return Some(response);
        }

        let body = match llm_fetch_models_payload(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/TestModels" {
        if let Some(response) = reject_non_post_method(method, "LLM/TestModels", is_head) {
            return Some(response);
        }

        let body = match llm_test_models_payload(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PreviewRequest" {
        if let Some(response) = reject_non_post_method(method, "LLM/PreviewRequest", is_head) {
            return Some(response);
        }

        let body = match llm_preview_request_payload(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "LLM/PromptProfiles only accepts GET");
            return Some(napcat_response(body, is_head));
        }

        let body = match llm_prompt_profiles_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles/Save" {
        if let Some(response) = reject_non_post_method(method, "LLM/PromptProfiles/Save", is_head) {
            return Some(response);
        }

        let body = match save_llm_prompt_profile(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles/Delete" {
        if let Some(response) = reject_non_post_method(method, "LLM/PromptProfiles/Delete", is_head)
        {
            return Some(response);
        }

        let body = match delete_llm_prompt_profile(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles/Use" {
        if let Some(response) = reject_non_post_method(method, "LLM/PromptProfiles/Use", is_head) {
            return Some(response);
        }

        let body = match use_llm_prompt_profile(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles/Preview" {
        if let Some(response) =
            reject_non_post_method(method, "LLM/PromptProfiles/Preview", is_head)
        {
            return Some(response);
        }

        let body = match preview_llm_prompt_profile(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    None
}

fn reject_non_post_method(method: &str, route_name: &str, is_head: bool) -> Option<Vec<u8>> {
    if method.eq_ignore_ascii_case("POST") {
        return None;
    }

    let body = napcat_err(-1, format!("{route_name} only accepts POST").as_str());
    Some(napcat_response(body, is_head))
}

fn llm_settings_payload() -> Result<Value, String> {
    let llm_config = current_llm_runtime_config()?;
    let prompt_profile = current_active_prompt_profile()?;
    let base_url = normalize_base_url(llm_config.base_url.as_str());
    let provider_id =
        resolve_provider_id(Some(llm_config.provider.as_str()), Some(base_url.as_str()));
    let provider_options = configured_provider_options(base_url.as_str(), provider_id.as_str())?;
    let model_options = model_options_for_provider(provider_id.as_str());
    let reasoning_options =
        reasoning_options_for_provider(provider_id.as_str(), llm_config.model.as_str());
    let supports = current_provider_supports(
        provider_id.as_str(),
        base_url.as_str(),
        reasoning_options.as_slice(),
    );

    Ok(json!({
        "enabled": llm_config.enabled,
        "provider": provider_label(provider_id.as_str()),
        "baseUrl": base_url,
        "model": llm_config.model,
        "providerOptions": provider_options,
        "modelOptions": model_options,
        "reasoningOptions": reasoning_options,
        "promptProfile": prompt_profile.name,
        "supports": supports,
        "providerCatalog": provider_catalog(),
    }))
}

fn llm_manager_state_payload() -> Result<Value, String> {
    let doc = load_current_app_config_doc()?;
    let runtime = current_llm_runtime_config()?;
    let providers = load_managed_providers_from_doc(&doc, &runtime);
    let active_provider_id = resolve_active_provider_id(
        doc.llm
            .as_ref()
            .and_then(|section| section.active_provider_id.clone()),
        &providers,
        runtime.base_url.as_str(),
    );
    let config_path = resolve_llm_config_write_path_for_web();

    Ok(json!({
        "activeProviderId": active_provider_id,
        "configPath": config_path.display().to_string(),
        "providerCatalog": provider_catalog(),
        "providers": providers
            .iter()
            .map(|provider| serialize_managed_provider(provider, active_provider_id.as_deref()))
            .collect::<Vec<_>>(),
    }))
}

fn save_llm_manager_state(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let save_request: WebLlmManagerSaveRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid LLM manager save payload: {err}"))?;

    let doc = load_current_app_config_doc()?;
    let runtime = current_llm_runtime_config()?;
    let providers = normalize_managed_provider_inputs(
        save_request.providers.as_slice(),
        runtime.timeout_ms.saturating_div(1000).max(1),
    );
    let active_provider_id = resolve_active_provider_id(
        save_request.active_provider_id,
        &providers,
        runtime.base_url.as_str(),
    );
    let active_provider = active_provider_id.as_deref().and_then(|active_id| {
        providers
            .iter()
            .find(|provider| provider.id.as_deref() == Some(active_id))
    });
    let active_model = active_provider.and_then(select_primary_model_id);
    let fallback_model = doc
        .llm
        .as_ref()
        .and_then(|section| section.model.clone())
        .unwrap_or_else(|| runtime.model.clone());
    let provider_urls = providers
        .iter()
        .filter_map(|provider| provider.base_url.clone())
        .collect::<Vec<_>>();
    let active_api_keys = active_provider
        .and_then(|provider| provider.api_key.clone())
        .map(|value| vec![value])
        .unwrap_or_default();
    let active_headers = active_provider
        .and_then(|provider| provider.headers.clone())
        .unwrap_or_default();
    let timeout_seconds = active_provider
        .and_then(|provider| provider.timeout_seconds)
        .or_else(|| {
            doc.llm
                .as_ref()
                .and_then(|section| section.timeout_seconds)
                .filter(|value| *value > 0)
        })
        .or(Some(runtime.timeout_ms.saturating_div(1000).max(1)));

    let patch = LlmConfigPatch {
        enabled: Some(active_provider.is_some() && active_model.is_some()),
        provider: active_provider.and_then(|provider| provider.provider.clone()),
        base_url: active_provider.and_then(|provider| provider.base_url.clone()),
        provider_urls: Some(provider_urls),
        model: Some(active_model.unwrap_or(fallback_model)),
        api_keys: Some(active_api_keys),
        timeout_seconds,
        headers: Some(active_headers),
        active_provider_id,
        providers: Some(providers),
        ..Default::default()
    };

    let path = resolve_llm_config_write_path_for_web();
    ensure_llm_config_file(path.as_path())?;
    crate::config_edit::persist_llm_config(path.as_path(), &patch)?;
    llm_manager_state_payload()
}

fn llm_fetch_models_payload(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmProviderActionRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid LLM provider payload: {err}"))?;
    let runtime = current_llm_runtime_config()?;
    let provider = normalize_single_provider_input(&payload.provider, 0, runtime.timeout_ms / 1000);
    let (models, source) = discover_models_for_provider(&provider)?;
    let enabled_models = provider_enabled_model_set(&provider);
    let merged_models =
        merge_saved_and_discovered_models(&provider, models.as_slice(), &enabled_models);

    Ok(json!({
        "source": source,
        "provider": serialize_managed_provider(&provider, None),
        "models": merged_models,
    }))
}

fn llm_test_models_payload(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmProviderActionRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid LLM provider test payload: {err}"))?;
    let runtime = current_llm_runtime_config()?;
    let provider = normalize_single_provider_input(&payload.provider, 0, runtime.timeout_ms / 1000);
    let model_ids =
        resolve_models_to_test(&provider, payload.model_id.as_deref(), payload.test_all);
    if model_ids.is_empty() {
        return Err("no provider model available for probing".to_string());
    }

    let results = model_ids
        .into_iter()
        .map(|model_id| probe_single_provider_model(&provider, model_id.as_str()))
        .collect::<Vec<_>>();

    Ok(json!({
        "provider": serialize_managed_provider(&provider, None),
        "results": results,
    }))
}

fn llm_preview_request_payload(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmProviderActionRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid LLM preview payload: {err}"))?;
    let runtime = current_llm_runtime_config()?;
    let provider = normalize_single_provider_input(&payload.provider, 0, runtime.timeout_ms / 1000);
    let model_id = payload
        .model_id
        .or_else(|| select_primary_model_id(&provider))
        .ok_or_else(|| "no model available for request preview".to_string())?;
    let prepared = prepare_provider_request(&provider, model_id.as_str())?;
    let preview_body = redact_preview_payload(&prepared.payload);

    Ok(json!({
        "provider": serialize_managed_provider(&provider, None),
        "modelId": model_id,
        "method": "POST",
        "endpoint": prepared.endpoint,
        "headers": preview_headers_map(
            prepared.api_key.as_deref(),
            prepared.auth.clone(),
            prepared.extra_headers.as_slice(),
        ),
        "body": preview_body,
        "bodyText": serde_json::to_string_pretty(&preview_body)
            .unwrap_or_else(|err| format!("<failed to render request body: {err}>")),
    }))
}

fn llm_prompt_profiles_payload() -> Result<Value, String> {
    let store = load_llm_prompt_store()?;
    Ok(serialize_prompt_store_payload(&store))
}

fn save_llm_prompt_profile(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmPromptProfileSaveRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid prompt profile save payload: {err}"))?;
    let mut store = load_llm_prompt_store()?;
    store.upsert_profile(payload.name.as_str(), payload.soul.as_str())?;
    if payload.active.unwrap_or(false) {
        store.set_active_profile(payload.name.as_str())?;
    }
    persist_llm_prompt_store(&store)?;
    Ok(serialize_prompt_store_payload(&store))
}

fn delete_llm_prompt_profile(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmPromptProfileNameRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid prompt profile delete payload: {err}"))?;
    let mut store = load_llm_prompt_store()?;
    store.remove_profile(payload.name.as_str())?;
    persist_llm_prompt_store(&store)?;
    Ok(serialize_prompt_store_payload(&store))
}

fn use_llm_prompt_profile(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmPromptProfileNameRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid prompt profile use payload: {err}"))?;
    let mut store = load_llm_prompt_store()?;
    store.set_active_profile(payload.name.as_str())?;
    persist_llm_prompt_store(&store)?;
    Ok(serialize_prompt_store_payload(&store))
}

fn preview_llm_prompt_profile(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmPromptProfilePreviewRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid prompt profile preview payload: {err}"))?;
    let store = load_llm_prompt_store()?;
    let profile = resolve_prompt_profile_for_preview(&store, payload.name.as_deref())?;
    let llm_config = current_llm_runtime_config()?;
    let preview = build_prompt_preview(
        payload
            .system_prompt
            .as_deref()
            .or(llm_config.system_prompt.as_deref()),
        payload.user_prompt.unwrap_or_default().as_str(),
        profile.soul.as_str(),
    );

    Ok(serialize_prompt_preview_payload(&store, &profile, &preview))
}

fn serialize_prompt_store_payload(store: &LlmPromptStore) -> Value {
    let store = store.normalized();
    let mut profiles = store
        .profiles
        .iter()
        .map(|profile| serialize_prompt_profile(profile, store.active_profile.as_str()))
        .collect::<Vec<_>>();
    profiles.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["name"].as_str().unwrap_or_default())
    });

    json!({
        "configPath": resolve_llm_prompt_store_path().display().to_string(),
        "activeProfile": store.active_profile,
        "profiles": profiles,
    })
}

fn serialize_prompt_preview_payload(
    store: &LlmPromptStore,
    profile: &LlmPromptProfile,
    preview: &LlmPromptPreview,
) -> Value {
    json!({
        "configPath": resolve_llm_prompt_store_path().display().to_string(),
        "activeProfile": store.normalized().active_profile,
        "profile": serialize_prompt_profile(profile, store.active_profile.as_str()),
        "preview": {
            "systemPrompt": preview.system_prompt,
            "composedUserPrompt": preview.composed_user_prompt,
            "combinedPrompt": preview.combined_prompt,
        }
    })
}

fn serialize_prompt_profile(profile: &LlmPromptProfile, active_profile: &str) -> Value {
    json!({
        "name": profile.name,
        "soul": profile.soul,
        "active": profile.name == active_profile,
        "canDelete": profile.name != "default",
    })
}

fn resolve_prompt_profile_for_preview(
    store: &LlmPromptStore,
    requested_name: Option<&str>,
) -> Result<LlmPromptProfile, String> {
    let normalized_name = requested_name
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(name) = normalized_name {
        return store
            .normalized()
            .profiles
            .into_iter()
            .find(|profile| profile.name == name)
            .ok_or_else(|| format!("prompt profile not found: {name}"));
    }
    Ok(store.active_profile())
}

fn llm_chat_payload(service: &WebHostService, request: &[u8]) -> Result<Value, String> {
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
        .map(str::trim)
        .filter(|value| !value.is_empty())
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

fn summarize_frontend_chat_request(
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

async fn execute_provider_chat(
    service: &WebHostService,
    request: &WebLlmChatRequest,
    llm_config: &crate::app_config::LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    provider_id: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    match provider_api_family(provider_id) {
        LlmProviderApiFamily::AnthropicMessages => {
            send_anthropic_chat(
                request,
                llm_config,
                api_key,
                soul,
                effective_base_url,
                effective_model,
                reasoning_effort,
            )
            .await
        }
        LlmProviderApiFamily::GeminiGenerateContent => {
            send_gemini_chat(
                request,
                llm_config,
                api_key,
                soul,
                effective_base_url,
                effective_model,
                reasoning_effort,
            )
            .await
        }
        LlmProviderApiFamily::ChatCompletions => {
            send_chat_completions_family_chat(
                service,
                request,
                llm_config,
                api_key,
                soul,
                effective_base_url,
                effective_model,
                reasoning_effort,
            )
            .await
        }
        LlmProviderApiFamily::Responses => {
            send_openai_compatible_chat(
                service,
                request,
                llm_config,
                api_key,
                soul,
                provider_id,
                effective_base_url,
                effective_model,
                reasoning_effort,
            )
            .await
        }
    }
}

async fn send_openai_compatible_chat(
    service: &WebHostService,
    request: &WebLlmChatRequest,
    llm_config: &crate::app_config::LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    provider_id: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    let fallback_prompt = compose_chat_fallback_prompt(request, soul)?;
    let responses_input = build_responses_input(request, soul, provider_id)?;
    let plugin_tools = runtime_plugin_tools(service)?;
    let capability_bundle = ToolManager::for_current_workspace()?
        .build_runtime_bundle(plugin_tools.as_slice())
        .await?;
    let effective_config = WebLlmRuntimeConfig {
        base_url: effective_base_url.to_string(),
        model: effective_model.to_string(),
        timeout_ms: llm_config.timeout_ms,
        system_prompt: merge_system_prompt_sections(
            llm_config.system_prompt.as_deref(),
            [capability_bundle.system_prompt.as_deref()],
        ),
        stream: false,
        temperature: request.temperature.or(llm_config.temperature),
        top_p: request.top_p.or(llm_config.top_p),
        top_k: request.top_k.or(llm_config.top_k),
        parallel_tool_calls: llm_config.parallel_tool_calls,
        reasoning_effort: reasoning_effort.map(ToString::to_string),
        default_headers: llm_config.headers.clone(),
    };

    let client = OpenAiResponsesClient::from_runtime_with_api_key(&effective_config, api_key)
        .map_err(|err: LlmClientError| err.to_string())?;
    let completion = client
        .complete_with_input(
            fallback_prompt.as_str(),
            responses_input,
            capability_bundle.tools.as_slice(),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;

    Ok(WebLlmChatExecution {
        message: completion.text,
        model: effective_model.to_string(),
        base_url: effective_base_url.to_string(),
    })
}

async fn send_chat_completions_family_chat(
    service: &WebHostService,
    request: &WebLlmChatRequest,
    llm_config: &crate::app_config::LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    let plugin_tools = runtime_plugin_tools(service)?;
    let capability_bundle = ToolManager::for_current_workspace()?
        .build_runtime_bundle(plugin_tools.as_slice())
        .await?;
    let merged_system_prompt = merge_system_prompt_sections(
        llm_config.system_prompt.as_deref(),
        [capability_bundle.system_prompt.as_deref()],
    );
    let messages = build_chat_completions_messages(request, merged_system_prompt.as_deref(), soul)?;
    let effective_config = WebLlmRuntimeConfig {
        base_url: effective_base_url.to_string(),
        model: effective_model.to_string(),
        timeout_ms: llm_config.timeout_ms,
        system_prompt: None,
        stream: false,
        temperature: request.temperature.or(llm_config.temperature),
        top_p: request.top_p.or(llm_config.top_p),
        top_k: request.top_k.or(llm_config.top_k),
        parallel_tool_calls: llm_config.parallel_tool_calls,
        reasoning_effort: reasoning_effort.map(ToString::to_string),
        default_headers: llm_config.headers.clone(),
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&effective_config, api_key)
        .map_err(|err: LlmClientError| err.to_string())?;
    let completion = client
        .complete_with_chat_messages(messages, capability_bundle.tools.as_slice(), None)
        .await
        .map_err(|err| err.to_string())?;

    Ok(WebLlmChatExecution {
        message: completion.text,
        model: effective_model.to_string(),
        base_url: effective_base_url.to_string(),
    })
}

fn runtime_plugin_tools(service: &WebHostService) -> Result<Vec<crate::LlmFunctionTool>, String> {
    match service.runtime_host.as_ref() {
        Some(runtime_host) => run_async_for_web_host(runtime_host.build_all_plugin_tool_bundle()),
        None => Ok(Vec::new()),
    }
}

async fn send_anthropic_chat(
    request: &WebLlmChatRequest,
    llm_config: &crate::app_config::LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    let endpoint = join_api_endpoint(effective_base_url, "v1/messages");
    let payload = build_anthropic_request(
        request,
        llm_config.system_prompt.as_deref(),
        soul,
        effective_model,
        request.temperature.or(llm_config.temperature),
        request.top_p.or(llm_config.top_p),
        request.top_k.or(llm_config.top_k),
        reasoning_effort,
    )?;
    let response = send_json_request(
        llm_config.timeout_ms,
        endpoint.as_str(),
        api_key,
        ProviderAuth::ApiKeyHeader("x-api-key".to_string()),
        &collect_runtime_headers(
            &llm_config.headers,
            &[("anthropic-version", DEFAULT_ANTHROPIC_API_VERSION)],
        ),
        &payload,
    )
    .await?;
    let message = extract_anthropic_text(&response)
        .ok_or_else(|| "Anthropic response did not contain assistant text".to_string())?;

    Ok(WebLlmChatExecution {
        message,
        model: response
            .pointer("/model")
            .and_then(Value::as_str)
            .unwrap_or(effective_model)
            .to_string(),
        base_url: effective_base_url.to_string(),
    })
}

async fn send_gemini_chat(
    request: &WebLlmChatRequest,
    llm_config: &crate::app_config::LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    let endpoint = join_api_endpoint(
        effective_base_url,
        format!("models/{effective_model}:generateContent").as_str(),
    );
    let payload = build_gemini_request(
        request,
        llm_config.system_prompt.as_deref(),
        soul,
        effective_model,
        request.temperature.or(llm_config.temperature),
        request.top_p.or(llm_config.top_p),
        request.top_k.or(llm_config.top_k),
        reasoning_effort,
    )?;
    let response = send_json_request(
        llm_config.timeout_ms,
        endpoint.as_str(),
        api_key,
        ProviderAuth::ApiKeyHeader("x-goog-api-key".to_string()),
        &collect_runtime_headers(&llm_config.headers, &[]),
        &payload,
    )
    .await?;
    let message = extract_gemini_text(&response)
        .ok_or_else(|| "Gemini response did not contain assistant text".to_string())?;

    Ok(WebLlmChatExecution {
        message,
        model: effective_model.to_string(),
        base_url: effective_base_url.to_string(),
    })
}

async fn send_json_request(
    timeout_ms: u64,
    endpoint: &str,
    api_key: &str,
    auth: ProviderAuth,
    extra_headers: &[(String, String)],
    payload: &Value,
) -> Result<Value, String> {
    log_provider_request_body(endpoint, payload);
    let client = build_provider_http_client(timeout_ms)?;
    let mut request = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json");

    request = match auth {
        ProviderAuth::Bearer => request.bearer_auth(api_key),
        ProviderAuth::ApiKeyHeader(name) => request.header(name, api_key),
    };

    for (name, value) in extra_headers {
        request = request.header(name, value);
    }

    let response = request
        .json(payload)
        .send()
        .await
        .map_err(|err| format!("failed to call upstream LLM provider: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("failed to read upstream LLM response: {err}"))?;

    if !status.is_success() {
        return Err(summarize_llm_upstream_error(status, body.as_str()));
    }

    serde_json::from_str(&body).map_err(|err| format!("upstream returned invalid JSON: {err}"))
}

async fn send_json_request_with_optional_auth(
    timeout_ms: u64,
    endpoint: &str,
    api_key: Option<&str>,
    auth: Option<ProviderAuth>,
    extra_headers: &[(String, String)],
    payload: &Value,
) -> Result<Value, String> {
    let mut request = build_provider_http_client(timeout_ms)?
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    request = apply_provider_auth(request, api_key, auth)?;
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }

    log_provider_request_body(endpoint, payload);
    let response = request
        .json(payload)
        .send()
        .await
        .map_err(|err| format!("failed to call upstream LLM provider: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("failed to read upstream LLM response: {err}"))?;

    if !status.is_success() {
        return Err(summarize_llm_upstream_error(status, body.as_str()));
    }

    serde_json::from_str(&body).map_err(|err| format!("upstream returned invalid JSON: {err}"))
}

async fn send_json_get_request(
    timeout_ms: u64,
    endpoint: &str,
    api_key: Option<&str>,
    auth: Option<ProviderAuth>,
    extra_headers: &[(String, String)],
) -> Result<Value, String> {
    let mut request = build_provider_http_client(timeout_ms)?.get(endpoint);
    request = apply_provider_auth(request, api_key, auth)?;
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("failed to call upstream LLM provider: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("failed to read upstream LLM response: {err}"))?;

    if !status.is_success() {
        return Err(summarize_llm_upstream_error(status, body.as_str()));
    }

    serde_json::from_str(&body).map_err(|err| format!("upstream returned invalid JSON: {err}"))
}

fn apply_provider_auth(
    request: reqwest::RequestBuilder,
    api_key: Option<&str>,
    auth: Option<ProviderAuth>,
) -> Result<reqwest::RequestBuilder, String> {
    match auth {
        Some(ProviderAuth::Bearer) => {
            let api_key = api_key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "provider api key is required for this action".to_string())?;
            Ok(request.bearer_auth(api_key))
        }
        Some(ProviderAuth::ApiKeyHeader(name)) => {
            let api_key = api_key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "provider api key is required for this action".to_string())?;
            Ok(request.header(name, api_key))
        }
        None => Ok(request),
    }
}

fn log_provider_request_body(endpoint: &str, payload: &Value) {
    let rendered = serde_json::to_string_pretty(payload)
        .unwrap_or_else(|err| format!("<failed to serialize request body: {err}>"));
    emit_console_log(
        LogLevel::Debug,
        "web.llm.request",
        format!("POST {endpoint}\n{rendered}"),
    );
}

fn build_provider_http_client(timeout_ms: u64) -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(250)))
        .user_agent("RsLiteyukiBot-WebHost/0.1")
        .build()
        .map_err(|err| format!("failed to build LLM http client: {err}"))
}

#[allow(dead_code)]
fn build_openrouter_request(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
    model: &str,
    temperature: Option<f32>,
    top_p: Option<f32>,
    reasoning_effort: Option<&str>,
) -> Result<Value, String> {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert(
        "messages".to_string(),
        Value::Array(build_openrouter_messages(
            request,
            runtime_system_prompt,
            soul,
        )?),
    );

    if let Some(temperature) = temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(reasoning_effort) = reasoning_effort {
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": reasoning_effort }),
        );
    }

    Ok(Value::Object(body))
}

fn build_chat_completions_messages(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
) -> Result<Vec<Value>, String> {
    build_openrouter_messages(request, runtime_system_prompt, soul)
}

fn build_openrouter_messages(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
) -> Result<Vec<Value>, String> {
    let mut messages = Vec::new();

    if let Some(system_instruction) =
        compose_system_instruction(request, runtime_system_prompt, soul)?
    {
        messages.push(json!({
            "role": "system",
            "content": system_instruction,
        }));
    }

    for message in effective_messages(request) {
        if normalize_message_role(message.role.as_str()) == "system" {
            continue;
        }
        if let Some(content) = build_openrouter_message_content(&message)? {
            messages.push(json!({
                "role": normalize_message_role(message.role.as_str()),
                "content": content,
            }));
        }
    }

    if messages.is_empty() {
        return Err("message or attachments are required".to_string());
    }
    Ok(messages)
}

fn build_openrouter_message_content(message: &WebLlmMessage) -> Result<Option<Value>, String> {
    let mut parts = Vec::new();
    let text = message.content.trim();
    if !text.is_empty() {
        parts.push(json!({
            "type": "text",
            "text": text,
        }));
    }

    for attachment in &message.attachments {
        match attachment.kind.trim().to_ascii_lowercase().as_str() {
            "image" => {
                let data_url = attachment
                    .data_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        format!("image attachment '{}' is missing dataUrl", attachment.name)
                    })?;
                parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": data_url },
                }));
            }
            "text" => {
                let text = attachment
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| format!("text attachment '{}' is empty", attachment.name))?;
                parts.push(json!({
                    "type": "text",
                    "text": format!(
                        "[Attachment: {}]\n{}",
                        non_empty_attachment_name(attachment),
                        text
                    ),
                }));
            }
            "file" => {
                return Err(format!(
                    "binary file attachment '{}' is not supported by the current backend route yet",
                    non_empty_attachment_name(attachment)
                ));
            }
            other => return Err(format!("unsupported attachment kind: {other}")),
        }
    }

    if parts.is_empty() {
        Ok(None)
    } else if parts.len() == 1
        && message.attachments.is_empty()
        && parts[0].get("type").and_then(Value::as_str) == Some("text")
    {
        Ok(Some(Value::String(text.to_string())))
    } else {
        Ok(Some(Value::Array(parts)))
    }
}

fn build_anthropic_request(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
    model: &str,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    reasoning_effort: Option<&str>,
) -> Result<Value, String> {
    let thinking_budget = reasoning_effort.and_then(anthropic_budget_tokens);
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert(
        "max_tokens".to_string(),
        json!(thinking_budget.unwrap_or(0) + DEFAULT_WEB_LLM_MAX_OUTPUT_TOKENS),
    );
    body.insert(
        "messages".to_string(),
        Value::Array(build_anthropic_messages(request)?),
    );

    if let Some(system_instruction) =
        compose_system_instruction(request, runtime_system_prompt, soul)?
    {
        body.insert("system".to_string(), Value::String(system_instruction));
    }

    if let Some(budget_tokens) = thinking_budget {
        body.insert(
            "thinking".to_string(),
            json!({
                "type": "enabled",
                "budget_tokens": budget_tokens,
            }),
        );
        if let Some(top_p) = top_p {
            body.insert("top_p".to_string(), json!(top_p.min(0.95)));
        }
    } else {
        if let Some(temperature) = temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(top_p) = top_p {
            body.insert("top_p".to_string(), json!(top_p));
        }
        if let Some(top_k) = top_k {
            body.insert("top_k".to_string(), json!(top_k));
        }
    }

    Ok(Value::Object(body))
}

fn build_anthropic_messages(request: &WebLlmChatRequest) -> Result<Vec<Value>, String> {
    let mut messages = Vec::new();

    for message in effective_messages(request) {
        let role = normalize_message_role(message.role.as_str());
        if role == "system" {
            continue;
        }
        if let Some(content) = build_anthropic_content_blocks(&message)? {
            messages.push(json!({
                "role": role,
                "content": content,
            }));
        }
    }

    if messages.is_empty() {
        return Err("message or attachments are required".to_string());
    }
    Ok(messages)
}

fn build_anthropic_content_blocks(message: &WebLlmMessage) -> Result<Option<Value>, String> {
    let mut blocks = Vec::new();
    let text = message.content.trim();
    if !text.is_empty() {
        blocks.push(json!({
            "type": "text",
            "text": text,
        }));
    }

    for attachment in &message.attachments {
        match attachment.kind.trim().to_ascii_lowercase().as_str() {
            "image" => {
                let parsed = parse_data_url(
                    attachment.data_url.as_deref().unwrap_or_default(),
                    attachment.media_type.as_deref(),
                )?;
                blocks.push(json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": parsed.media_type,
                        "data": parsed.data,
                    }
                }));
            }
            "text" => {
                let text = attachment
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| format!("text attachment '{}' is empty", attachment.name))?;
                blocks.push(json!({
                    "type": "text",
                    "text": format!(
                        "[Attachment: {}]\n{}",
                        non_empty_attachment_name(attachment),
                        text
                    ),
                }));
            }
            "file" => {
                return Err(format!(
                    "binary file attachment '{}' is not supported by the current backend route yet",
                    non_empty_attachment_name(attachment)
                ));
            }
            other => return Err(format!("unsupported attachment kind: {other}")),
        }
    }

    if blocks.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Value::Array(blocks)))
    }
}

fn build_gemini_request(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
    model: &str,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    reasoning_effort: Option<&str>,
) -> Result<Value, String> {
    let mut body = Map::new();
    body.insert(
        "contents".to_string(),
        Value::Array(build_gemini_contents(request)?),
    );

    let mut generation_config = Map::new();
    if let Some(temperature) = temperature {
        generation_config.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = top_p {
        generation_config.insert("topP".to_string(), json!(top_p));
    }
    if let Some(top_k) = top_k {
        generation_config.insert("topK".to_string(), json!(top_k));
    }

    if let Some(thinking_config) = gemini_thinking_config(model, reasoning_effort) {
        generation_config.insert("thinkingConfig".to_string(), thinking_config);
    }

    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }

    if let Some(system_instruction) =
        compose_system_instruction(request, runtime_system_prompt, soul)?
    {
        body.insert(
            "system_instruction".to_string(),
            json!({
                "parts": [{ "text": system_instruction }],
            }),
        );
    }

    Ok(Value::Object(body))
}

fn build_gemini_contents(request: &WebLlmChatRequest) -> Result<Vec<Value>, String> {
    let mut contents = Vec::new();

    for message in effective_messages(request) {
        let role = normalize_message_role(message.role.as_str());
        if role == "system" {
            continue;
        }
        if let Some(parts) = build_gemini_parts(&message)? {
            contents.push(json!({
                "role": if role == "assistant" { "model" } else { "user" },
                "parts": parts,
            }));
        }
    }

    if contents.is_empty() {
        return Err("message or attachments are required".to_string());
    }
    Ok(contents)
}

fn build_gemini_parts(message: &WebLlmMessage) -> Result<Option<Value>, String> {
    let mut parts = Vec::new();
    let text = message.content.trim();
    if !text.is_empty() {
        parts.push(json!({ "text": text }));
    }

    for attachment in &message.attachments {
        match attachment.kind.trim().to_ascii_lowercase().as_str() {
            "image" => {
                let parsed = parse_data_url(
                    attachment.data_url.as_deref().unwrap_or_default(),
                    attachment.media_type.as_deref(),
                )?;
                parts.push(json!({
                    "inline_data": {
                        "mime_type": parsed.media_type,
                        "data": parsed.data,
                    }
                }));
            }
            "text" => {
                let text = attachment
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| format!("text attachment '{}' is empty", attachment.name))?;
                parts.push(json!({
                    "text": format!(
                        "[Attachment: {}]\n{}",
                        non_empty_attachment_name(attachment),
                        text
                    ),
                }));
            }
            "file" => {
                return Err(format!(
                    "binary file attachment '{}' is not supported by the current backend route yet",
                    non_empty_attachment_name(attachment)
                ));
            }
            other => return Err(format!("unsupported attachment kind: {other}")),
        }
    }

    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Value::Array(parts)))
    }
}

fn compose_system_instruction(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
) -> Result<Option<String>, String> {
    let mut sections = Vec::new();

    if let Some(system_prompt) = runtime_system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(system_prompt.to_string());
    }

    let soul = soul.trim();
    if !soul.is_empty() {
        sections.push(format!("Prompt profile instruction:\n{soul}"));
    }

    for message in effective_messages(request) {
        if normalize_message_role(message.role.as_str()) != "system" {
            continue;
        }
        let content = message.content.trim();
        if !content.is_empty() {
            sections.push(format!("System message:\n{content}"));
        }
        let attachment_lines = attachment_summary_lines(message.attachments.as_slice())?;
        if !attachment_lines.is_empty() {
            sections.push(format!(
                "System attachments:\n- {}",
                attachment_lines.join("\n- ")
            ));
        }
    }

    if sections.is_empty() {
        Ok(None)
    } else {
        Ok(Some(sections.join("\n\n")))
    }
}

#[allow(dead_code)]
fn extract_openrouter_text(payload: &Value) -> Option<String> {
    extract_output_text(payload)
}

fn extract_anthropic_text(payload: &Value) -> Option<String> {
    let mut fragments = Vec::new();
    for item in payload.get("content").and_then(Value::as_array)? {
        if item.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = item.get("text").and_then(Value::as_str)
        {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                fragments.push(trimmed.to_string());
            }
        }
    }
    if fragments.is_empty() {
        None
    } else {
        Some(fragments.join("\n"))
    }
}

fn extract_gemini_text(payload: &Value) -> Option<String> {
    let mut fragments = Vec::new();
    for part in payload
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)?
    {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                fragments.push(trimmed.to_string());
            }
        }
    }
    if fragments.is_empty() {
        None
    } else {
        Some(fragments.join("\n"))
    }
}

fn anthropic_budget_tokens(reasoning_effort: &str) -> Option<u32> {
    Some(
        match reasoning_effort.trim().to_ascii_lowercase().as_str() {
            "minimal" | "low" => 1_024,
            "medium" => 4_096,
            "high" => 8_192,
            "xhigh" | "max" => 16_384,
            _ => return None,
        },
    )
}

fn gemini_thinking_config(model: &str, reasoning_effort: Option<&str>) -> Option<Value> {
    let reasoning_effort = reasoning_effort?;
    let model = model.to_ascii_lowercase();

    if model.starts_with("gemini-3") {
        let thinking_level = match reasoning_effort.trim().to_ascii_lowercase().as_str() {
            "minimal" | "low" => "low",
            "medium" => "medium",
            "high" | "xhigh" | "max" => "high",
            _ => return None,
        };
        return Some(json!({ "thinkingLevel": thinking_level }));
    }

    if model.starts_with("gemini-2.5") {
        let thinking_budget = match reasoning_effort.trim().to_ascii_lowercase().as_str() {
            "minimal" if model.contains("flash") => 0,
            "minimal" | "low" => 1_024,
            "medium" => 4_096,
            "high" => 8_192,
            "xhigh" | "max" => 24_576,
            _ => return None,
        };
        return Some(json!({ "thinkingBudget": thinking_budget }));
    }

    None
}

fn summarize_llm_upstream_error(status: StatusCode, body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .or_else(|| value.pointer("/error/details").and_then(Value::as_str))
                .or_else(|| value.pointer("/error/type").and_then(Value::as_str))
        })
        .unwrap_or_else(|| body.trim());
    let preview = truncate_inline(message, 240);
    format!("upstream returned {}: {}", status.as_u16(), preview)
}

fn join_api_endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim().trim_end_matches('/'),
        path.trim().trim_start_matches('/')
    )
}

fn parse_data_url(raw: &str, fallback_media_type: Option<&str>) -> Result<ParsedDataUrl, String> {
    let raw = raw.trim();
    let (meta, data) = raw
        .split_once(',')
        .ok_or_else(|| "attachment dataUrl is malformed".to_string())?;
    let meta = meta
        .strip_prefix("data:")
        .ok_or_else(|| "attachment dataUrl must start with data:".to_string())?;
    let media_type = meta
        .split(';')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            fallback_media_type
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("application/octet-stream")
        .to_string();

    if !meta.to_ascii_lowercase().contains(";base64") {
        return Err("attachment dataUrl must use base64 encoding".to_string());
    }
    if data.trim().is_empty() {
        return Err("attachment dataUrl payload is empty".to_string());
    }

    Ok(ParsedDataUrl {
        media_type,
        data: data.trim().to_string(),
    })
}

#[derive(Debug, Clone, Deserialize, Default)]
struct WebLlmChatRequest {
    #[serde(default)]
    message: String,
    #[serde(default)]
    messages: Vec<WebLlmMessage>,
    #[serde(default)]
    attachments: Vec<WebLlmAttachment>,
    #[serde(default, rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "reasoningEffort")]
    reasoning_effort: Option<String>,
    #[serde(default, rename = "temperature")]
    temperature: Option<f32>,
    #[serde(default, rename = "topP")]
    top_p: Option<f32>,
    #[serde(default, rename = "topK")]
    top_k: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct WebLlmMessage {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    attachments: Vec<WebLlmAttachment>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
struct WebLlmAttachment {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "mediaType")]
    media_type: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default, rename = "dataUrl")]
    data_url: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Clone)]
struct WebLlmRuntimeConfig {
    base_url: String,
    model: String,
    timeout_ms: u64,
    system_prompt: Option<String>,
    stream: bool,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    parallel_tool_calls: bool,
    reasoning_effort: Option<String>,
    default_headers: HashMap<String, String>,
}

impl OpenAiRuntimeConfig for WebLlmRuntimeConfig {
    fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    fn model(&self) -> &str {
        self.model.as_str()
    }

    fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    fn stream(&self) -> bool {
        self.stream
    }

    fn temperature(&self) -> Option<f32> {
        self.temperature
    }

    fn top_p(&self) -> Option<f32> {
        self.top_p
    }

    fn top_k(&self) -> Option<u32> {
        self.top_k
    }

    fn parallel_tool_calls(&self) -> bool {
        self.parallel_tool_calls
    }

    fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    fn default_headers(&self) -> Option<&HashMap<String, String>> {
        Some(&self.default_headers)
    }
}

#[derive(Debug, Clone)]
struct WebLlmChatExecution {
    message: String,
    model: String,
    base_url: String,
}

#[derive(Debug, Clone)]
struct ParsedDataUrl {
    media_type: String,
    data: String,
}

#[derive(Debug, Clone)]
enum ProviderAuth {
    Bearer,
    ApiKeyHeader(String),
}

fn pick_llm_api_key(llm_config: &crate::app_config::LlmRuntimeConfig) -> Result<String, String> {
    let index = next_llm_api_key_index(llm_config.api_keys.len())
        .ok_or_else(|| "no api key configured".to_string())?;
    llm_config
        .api_keys
        .get(index)
        .cloned()
        .ok_or_else(|| "no api key configured".to_string())
}

fn validate_sampling_args(
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
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
    Ok(())
}

fn build_responses_input(
    request: &WebLlmChatRequest,
    soul: &str,
    provider_id: &str,
) -> Result<Value, String> {
    let text = compose_chat_fallback_prompt(request, soul)?;
    let mut content = vec![json!({
        "type": "input_text",
        "text": text,
    })];

    for attachment in latest_turn_attachments(request) {
        match attachment.kind.trim().to_ascii_lowercase().as_str() {
            "image" => {
                let data_url = attachment
                    .data_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        format!("image attachment '{}' is missing dataUrl", attachment.name)
                    })?;
                if provider_id != "openai" {
                    return Err(
                        "current provider route does not support image input yet".to_string()
                    );
                }
                content.push(json!({
                    "type": "input_image",
                    "image_url": data_url,
                }));
            }
            "text" => {
                let text = attachment
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| format!("text attachment '{}' is empty", attachment.name))?;
                content.push(json!({
                    "type": "input_text",
                    "text": format!(
                        "[Attachment: {}]\n{}",
                        non_empty_attachment_name(attachment),
                        text
                    ),
                }));
            }
            "file" => {
                return Err(format!(
                    "binary file attachment '{}' is not supported by the current backend route yet",
                    non_empty_attachment_name(attachment)
                ));
            }
            other => return Err(format!("unsupported attachment kind: {other}")),
        }
    }

    Ok(json!([{
        "role": "user",
        "content": content,
    }]))
}

fn compose_chat_fallback_prompt(request: &WebLlmChatRequest, soul: &str) -> Result<String, String> {
    let mut sections = Vec::new();
    let soul = soul.trim();
    if !soul.is_empty() {
        sections.push(format!("Prompt profile instruction:\n{soul}"));
    }

    let messages = effective_messages(request);
    if messages.is_empty() {
        return Err("message or attachments are required".to_string());
    }

    let mut transcript = String::new();
    for message in &messages {
        let role = normalize_message_role(message.role.as_str());
        let content = message.content.trim();
        if !content.is_empty() {
            transcript.push_str(role);
            transcript.push_str(":\n");
            transcript.push_str(content);
            transcript.push_str("\n\n");
        }
        let attachment_lines = attachment_summary_lines(message.attachments.as_slice())?;
        if !attachment_lines.is_empty() {
            transcript.push_str(role);
            transcript.push_str(" attachments:\n");
            for line in attachment_lines {
                transcript.push_str("- ");
                transcript.push_str(line.as_str());
                transcript.push('\n');
            }
            transcript.push('\n');
        }
    }

    if transcript.trim().is_empty() {
        return Err("message or attachments are required".to_string());
    }
    sections.push(format!("Conversation transcript:\n{}", transcript.trim()));

    Ok(sections.join("\n\n"))
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WebLlmManagerSaveRequest {
    #[serde(default)]
    active_provider_id: Option<String>,
    #[serde(default)]
    providers: Vec<WebLlmManagedProviderPayload>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WebLlmPromptProfileSaveRequest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    soul: String,
    #[serde(default)]
    active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WebLlmPromptProfileNameRequest {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WebLlmPromptProfilePreviewRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    user_prompt: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WebLlmProviderActionRequest {
    #[serde(default)]
    provider: WebLlmManagedProviderPayload,
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    test_all: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WebLlmManagedProviderPayload {
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    models: Vec<WebLlmManagedModelPayload>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WebLlmManagedModelPayload {
    #[serde(default)]
    id: String,
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Clone)]
struct PreparedProviderRequest {
    endpoint: String,
    auth: Option<ProviderAuth>,
    api_key: Option<String>,
    extra_headers: Vec<(String, String)>,
    payload: Value,
}

fn resolve_llm_config_write_path_for_web() -> std::path::PathBuf {
    resolve_llm_config_path().unwrap_or_else(resolve_default_llm_config_path)
}

fn load_managed_providers_from_doc(
    doc: &crate::app_config::AppConfigDoc,
    runtime: &crate::app_config::LlmRuntimeConfig,
) -> Vec<LlmManagedProviderConfig> {
    if let Some(saved) = doc
        .llm
        .as_ref()
        .and_then(|section| section.providers.clone())
        .filter(|providers| !providers.is_empty())
    {
        let fallback_timeout = doc
            .llm
            .as_ref()
            .and_then(|section| section.timeout_seconds)
            .filter(|value| *value > 0)
            .unwrap_or_else(|| runtime.timeout_ms.saturating_div(1000).max(1));
        return saved
            .iter()
            .enumerate()
            .map(|(index, provider)| {
                enrich_saved_provider(provider, index, fallback_timeout, runtime)
            })
            .collect();
    }

    vec![LlmManagedProviderConfig {
        id: Some(derive_provider_entry_id(
            None,
            Some(runtime.provider.as_str()),
            runtime.base_url.as_str(),
            0,
        )),
        label: Some(
            provider_label(
                resolve_provider_id(
                    Some(runtime.provider.as_str()),
                    Some(runtime.base_url.as_str()),
                )
                .as_str(),
            )
            .to_string(),
        ),
        provider: Some(resolve_provider_id(
            Some(runtime.provider.as_str()),
            Some(runtime.base_url.as_str()),
        )),
        base_url: Some(normalize_base_url(runtime.base_url.as_str())),
        api_key: runtime.api_keys.first().cloned(),
        timeout_seconds: Some(runtime.timeout_ms.saturating_div(1000).max(1)),
        headers: Some(runtime.headers.clone()),
        models: Some(vec![LlmManagedModelConfig {
            id: Some(runtime.model.clone()),
            enabled: Some(true),
        }]),
    }]
}

fn enrich_saved_provider(
    provider: &LlmManagedProviderConfig,
    index: usize,
    fallback_timeout_seconds: u64,
    runtime: &crate::app_config::LlmRuntimeConfig,
) -> LlmManagedProviderConfig {
    let base_url = provider
        .base_url
        .as_deref()
        .map(normalize_base_url)
        .filter(|value| !value.is_empty())
        .or_else(|| Some(normalize_base_url(runtime.base_url.as_str())));
    let provider_id = provider
        .provider
        .as_deref()
        .or(base_url.as_deref())
        .map(|_| resolve_provider_id(provider.provider.as_deref(), base_url.as_deref()));
    let id = Some(derive_provider_entry_id(
        provider.id.as_deref(),
        provider.label.as_deref(),
        base_url.as_deref().unwrap_or(runtime.base_url.as_str()),
        index,
    ));
    let label = normalize_optional_string(provider.label.as_deref())
        .or_else(|| id.clone())
        .or_else(|| {
            provider_id
                .as_deref()
                .map(provider_label)
                .map(ToString::to_string)
        });
    let api_key = normalize_optional_string(provider.api_key.as_deref()).or_else(|| {
        if base_url.as_deref() == Some(runtime.base_url.as_str()) {
            runtime.api_keys.first().cloned()
        } else {
            None
        }
    });
    let headers = provider.headers.clone().unwrap_or_else(|| {
        if base_url.as_deref() == Some(runtime.base_url.as_str()) {
            runtime.headers.clone()
        } else {
            HashMap::new()
        }
    });
    let models = provider
        .models
        .clone()
        .filter(|models| !models.is_empty())
        .or_else(|| {
            if base_url.as_deref() == Some(runtime.base_url.as_str()) {
                Some(vec![LlmManagedModelConfig {
                    id: Some(runtime.model.clone()),
                    enabled: Some(true),
                }])
            } else {
                None
            }
        });

    LlmManagedProviderConfig {
        id,
        label,
        provider: provider_id,
        base_url,
        api_key,
        timeout_seconds: provider
            .timeout_seconds
            .filter(|value| *value > 0)
            .or(Some(fallback_timeout_seconds.max(1))),
        headers: Some(headers),
        models,
    }
}

fn resolve_active_provider_id(
    explicit_active_provider_id: Option<String>,
    providers: &[LlmManagedProviderConfig],
    active_base_url: &str,
) -> Option<String> {
    let explicit_active_provider_id = explicit_active_provider_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(explicit_active_provider_id) = explicit_active_provider_id
        && providers
            .iter()
            .any(|provider| provider.id.as_deref() == Some(explicit_active_provider_id))
    {
        return Some(explicit_active_provider_id.to_string());
    }

    providers
        .iter()
        .find(|provider| provider.base_url.as_deref() == Some(active_base_url))
        .and_then(|provider| provider.id.clone())
        .or_else(|| providers.first().and_then(|provider| provider.id.clone()))
}

fn serialize_managed_provider(
    provider: &LlmManagedProviderConfig,
    active_provider_id: Option<&str>,
) -> Value {
    let provider_id = provider
        .provider
        .as_deref()
        .or(provider.base_url.as_deref())
        .map(|_| resolve_provider_id(provider.provider.as_deref(), provider.base_url.as_deref()))
        .unwrap_or_else(|| "openai-compatible".to_string());
    let label = provider
        .label
        .clone()
        .or_else(|| provider.id.clone())
        .unwrap_or_else(|| provider_label(provider_id.as_str()).to_string());
    let headers = provider.headers.clone().unwrap_or_default();
    let models = provider
        .models
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|model| {
            model.id.map(|id| {
                json!({
                    "id": id,
                    "enabled": model.enabled.unwrap_or(false),
                })
            })
        })
        .collect::<Vec<_>>();

    json!({
        "id": provider.id.clone().unwrap_or_default(),
        "label": label,
        "providerId": provider_id.clone(),
        "providerLabel": provider_label(provider_id.as_str()),
        "baseUrl": provider.base_url.clone().unwrap_or_default(),
        "apiKey": provider.api_key.clone().unwrap_or_default(),
        "timeoutSeconds": provider.timeout_seconds.unwrap_or(120),
        "headers": headers,
        "models": models,
        "active": active_provider_id.is_some_and(|active_id| provider.id.as_deref() == Some(active_id)),
    })
}

fn normalize_managed_provider_inputs(
    providers: &[WebLlmManagedProviderPayload],
    fallback_timeout_seconds: u64,
) -> Vec<LlmManagedProviderConfig> {
    providers
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            normalize_single_provider_input(provider, index, fallback_timeout_seconds)
        })
        .collect()
}

fn normalize_single_provider_input(
    provider: &WebLlmManagedProviderPayload,
    index: usize,
    fallback_timeout_seconds: u64,
) -> LlmManagedProviderConfig {
    let base_url = normalize_base_url(provider.base_url.as_str());
    let provider_id = provider
        .provider_id
        .as_deref()
        .or(Some(base_url.as_str()))
        .map(|_| resolve_provider_id(provider.provider_id.as_deref(), Some(base_url.as_str())))
        .unwrap_or_else(|| "openai-compatible".to_string());
    let id = derive_provider_entry_id(
        Some(provider.id.as_str()),
        Some(provider.label.as_str()),
        base_url.as_str(),
        index,
    );
    let label =
        normalize_optional_string(Some(provider.label.as_str())).unwrap_or_else(|| id.clone());
    let mut headers = provider
        .headers
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if key.is_empty() || value.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect::<HashMap<_, _>>();
    if headers.is_empty() {
        headers = HashMap::new();
    }
    let models = provider
        .models
        .iter()
        .filter_map(|model| {
            let id = model.id.trim();
            if id.is_empty() {
                None
            } else {
                Some(LlmManagedModelConfig {
                    id: Some(id.to_string()),
                    enabled: Some(model.enabled),
                })
            }
        })
        .collect::<Vec<_>>();

    LlmManagedProviderConfig {
        id: Some(id),
        label: Some(label),
        provider: Some(provider_id),
        base_url: Some(base_url),
        api_key: normalize_optional_string(Some(provider.api_key.as_str())),
        timeout_seconds: Some(
            provider
                .timeout_seconds
                .unwrap_or(fallback_timeout_seconds.max(1))
                .max(1),
        ),
        headers: Some(headers),
        models: Some(models),
    }
}

fn derive_provider_entry_id(
    explicit_id: Option<&str>,
    label: Option<&str>,
    base_url: &str,
    index: usize,
) -> String {
    let raw = explicit_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| label.map(str::trim).filter(|value| !value.is_empty()))
        .unwrap_or(base_url);
    let slug = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        format!("provider-{}", index + 1)
    } else {
        slug
    }
}

fn normalize_optional_string(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn provider_enabled_model_set(
    provider: &LlmManagedProviderConfig,
) -> std::collections::HashSet<String> {
    provider
        .models
        .as_ref()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            model
                .enabled
                .unwrap_or(false)
                .then(|| model.id.clone())
                .flatten()
        })
        .collect()
}

fn merge_saved_and_discovered_models(
    provider: &LlmManagedProviderConfig,
    discovered_models: &[String],
    enabled_models: &std::collections::HashSet<String>,
) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut merged = Vec::new();

    for model_id in discovered_models {
        if seen.insert(model_id.clone()) {
            merged.push(json!({
                "id": model_id,
                "enabled": enabled_models.contains(model_id),
            }));
        }
    }

    if let Some(saved_models) = provider.models.as_ref() {
        for saved_model in saved_models {
            let Some(model_id) = saved_model.id.as_ref() else {
                continue;
            };
            if seen.insert(model_id.clone()) {
                merged.push(json!({
                    "id": model_id,
                    "enabled": saved_model.enabled.unwrap_or(false),
                }));
            }
        }
    }

    merged
}

fn resolve_models_to_test(
    provider: &LlmManagedProviderConfig,
    explicit_model_id: Option<&str>,
    test_all: bool,
) -> Vec<String> {
    if let Some(explicit_model_id) = explicit_model_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return vec![explicit_model_id.to_string()];
    }

    if test_all {
        return provider
            .models
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|model| model.id.clone())
            .collect();
    }

    select_primary_model_id(provider).into_iter().collect()
}

fn select_primary_model_id(provider: &LlmManagedProviderConfig) -> Option<String> {
    provider
        .models
        .as_ref()
        .and_then(|models| {
            models
                .iter()
                .find(|model| model.enabled.unwrap_or(false))
                .or_else(|| models.first())
        })
        .and_then(|model| model.id.clone())
}

fn discover_models_for_provider(
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

fn probe_single_provider_model(provider: &LlmManagedProviderConfig, model_id: &str) -> Value {
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

fn prepare_provider_request(
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
            payload: build_chat_completions_request(
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
            payload: build_openai_compatible_request(
                &chat_request,
                &runtime,
                prompt_profile.soul.as_str(),
                provider_id.as_str(),
            )?,
        }),
    }
}

fn build_chat_completions_request(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
    runtime: &WebLlmRuntimeConfig,
) -> Result<Value, String> {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(runtime.model.clone()));
    body.insert(
        "messages".to_string(),
        Value::Array(build_chat_completions_messages(
            request,
            runtime_system_prompt,
            soul,
        )?),
    );
    if let Some(temperature) = runtime.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = runtime.top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(top_k) = runtime.top_k {
        body.insert("top_k".to_string(), json!(top_k));
    }
    if let Some(reasoning_effort) = runtime.reasoning_effort.as_deref() {
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": reasoning_effort }),
        );
    }
    Ok(Value::Object(body))
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
    }
}

fn build_openai_compatible_request(
    request: &WebLlmChatRequest,
    runtime: &WebLlmRuntimeConfig,
    soul: &str,
    provider_id: &str,
) -> Result<Value, String> {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(runtime.model.clone()));
    let responses_input = build_responses_input(request, soul, provider_id)?;
    body.insert("input".to_string(), responses_input);
    if let Some(instructions) = runtime
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "instructions".to_string(),
            Value::String(instructions.to_string()),
        );
    }
    if let Some(temperature) = runtime.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = runtime.top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(top_k) = runtime.top_k.filter(|_| {
        !runtime
            .base_url
            .to_ascii_lowercase()
            .contains("api.openai.com")
    }) {
        body.insert("top_k".to_string(), json!(top_k));
    }
    if let Some(reasoning_effort) = runtime.reasoning_effort.as_deref() {
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": reasoning_effort }),
        );
    }
    Ok(Value::Object(body))
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

fn collect_runtime_headers(
    headers: &HashMap<String, String>,
    extra_headers: &[(&str, &str)],
) -> Vec<(String, String)> {
    let mut collected = headers
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    collected.extend(
        extra_headers
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string())),
    );
    collected
}

fn preview_headers_map(
    api_key: Option<&str>,
    auth: Option<ProviderAuth>,
    extra_headers: &[(String, String)],
) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    if let Some(auth) = auth {
        match auth {
            ProviderAuth::Bearer => {
                headers.insert(
                    "Authorization".to_string(),
                    format!("Bearer {}", mask_secret(api_key.unwrap_or_default())),
                );
            }
            ProviderAuth::ApiKeyHeader(name) => {
                headers.insert(name, mask_secret(api_key.unwrap_or_default()));
            }
        }
    }
    for (name, value) in extra_headers {
        headers.insert(name.clone(), value.clone());
    }
    headers
}

fn mask_secret(secret: &str) -> String {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let prefix: String = trimmed.chars().take(6).collect();
    let suffix: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{prefix}***{suffix}")
}

fn redact_preview_payload(payload: &Value) -> Value {
    match payload {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let redacted = match key.as_str() {
                        "instructions" | "text" => Value::String("<redacted>".to_string()),
                        _ => redact_preview_payload(value),
                    };
                    (key.clone(), redacted)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_preview_payload).collect()),
        _ => payload.clone(),
    }
}

fn effective_messages(request: &WebLlmChatRequest) -> Vec<WebLlmMessage> {
    if !request.messages.is_empty() {
        let mut messages = request.messages.clone();
        if !request.attachments.is_empty()
            && let Some(last) = messages.last_mut()
            && last.attachments.is_empty()
        {
            last.attachments = request.attachments.clone();
        }
        if !request.message.trim().is_empty()
            && let Some(last) = messages.last_mut()
            && last.content.trim().is_empty()
        {
            last.content = request.message.clone();
        }
        return messages;
    }

    vec![WebLlmMessage {
        role: "user".to_string(),
        content: request.message.clone(),
        attachments: request.attachments.clone(),
    }]
}

fn latest_turn_attachments(request: &WebLlmChatRequest) -> &[WebLlmAttachment] {
    if !request.attachments.is_empty() {
        request.attachments.as_slice()
    } else {
        request
            .messages
            .last()
            .map(|message| message.attachments.as_slice())
            .unwrap_or(&[])
    }
}

fn attachment_summary_lines(attachments: &[WebLlmAttachment]) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for attachment in attachments {
        let size_suffix = attachment
            .size
            .map(|size| format!(", {size} bytes"))
            .unwrap_or_default();
        match attachment.kind.trim().to_ascii_lowercase().as_str() {
            "image" => lines.push(format!(
                "image '{}' ({}{})",
                non_empty_attachment_name(attachment),
                attachment.media_type.as_deref().unwrap_or("unknown"),
                size_suffix
            )),
            "text" => {
                let text = attachment
                    .text
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default();
                lines.push(format!(
                    "text '{}'{}",
                    non_empty_attachment_name(attachment),
                    if text.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", truncate_inline(text, 240))
                    }
                ));
            }
            "file" => lines.push(format!(
                "file '{}' ({}{})",
                non_empty_attachment_name(attachment),
                attachment.media_type.as_deref().unwrap_or("unknown"),
                size_suffix
            )),
            other => return Err(format!("unsupported attachment kind: {other}")),
        }
    }
    Ok(lines)
}

fn truncate_inline(raw: &str, limit: usize) -> String {
    let mut chars = raw.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn non_empty_attachment_name(attachment: &WebLlmAttachment) -> String {
    let name = attachment.name.trim();
    if name.is_empty() {
        "unnamed".to_string()
    } else {
        name.to_string()
    }
}

fn normalize_message_role(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "assistant" => "assistant",
        "system" => "system",
        _ => "user",
    }
}

fn normalize_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn detect_provider_id(base_url: &str) -> String {
    detect_provider_id_from_base_url(base_url)
}

fn provider_label(provider_id: &str) -> &'static str {
    match provider_id {
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "google-gemini" => "Google Gemini",
        "openrouter" => "OpenRouter",
        "kimi" => "Kimi API",
        "qwen" => "Qwen API",
        _ => "OpenAI Compatible",
    }
}

fn configured_provider_options(
    active_base_url: &str,
    active_provider_id: &str,
) -> Result<Vec<Value>, String> {
    let doc = crate::llm::service::load_current_app_config_doc()?;
    let runtime = current_llm_runtime_config()?;
    let managed_providers = load_managed_providers_from_doc(&doc, &runtime);
    let mut urls = doc
        .llm
        .as_ref()
        .and_then(|section| section.provider_urls.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|url| normalize_base_url(url.as_str()))
        .filter(|url| !url.is_empty())
        .collect::<Vec<_>>();
    if urls.is_empty() {
        urls.push(active_base_url.to_string());
    }

    Ok(urls
        .into_iter()
        .enumerate()
        .map(|(index, url)| {
            let saved_provider_id = managed_providers
                .iter()
                .find(|provider| provider.base_url.as_deref() == Some(url.as_str()))
                .and_then(|provider| provider.provider.as_deref());
            let option_provider_id = if let Some(saved_provider_id) = saved_provider_id {
                resolve_provider_id(Some(saved_provider_id), Some(url.as_str()))
            } else if url == active_base_url {
                resolve_provider_id(Some(active_provider_id), Some(url.as_str()))
            } else {
                resolve_provider_id(None, Some(url.as_str()))
            };
            json!({
                "id": format!("{option_provider_id}-{index}"),
                "label": provider_label(option_provider_id.as_str()),
                "baseUrl": url,
                "active": url == active_base_url,
            })
        })
        .collect())
}

fn current_provider_supports(
    provider_id: &str,
    base_url: &str,
    reasoning_options: &[String],
) -> Value {
    match provider_id {
        "openai" => json!({
            "streaming": true,
            "temperature": true,
            "topP": true,
            "topK": false,
            "reasoningEffort": !reasoning_options.is_empty(),
            "imageInput": true,
            "textFileInput": true,
            "binaryFileInput": false
        }),
        "anthropic" => json!({
            "streaming": true,
            "temperature": true,
            "topP": true,
            "topK": true,
            "reasoningEffort": false,
            "imageInput": true,
            "textFileInput": true,
            "binaryFileInput": false
        }),
        "google-gemini" => json!({
            "streaming": true,
            "temperature": true,
            "topP": true,
            "topK": true,
            "reasoningEffort": false,
            "imageInput": true,
            "textFileInput": true,
            "binaryFileInput": false
        }),
        "openrouter" => json!({
            "streaming": true,
            "temperature": true,
            "topP": true,
            "topK": false,
            "reasoningEffort": false,
            "imageInput": true,
            "textFileInput": true,
            "binaryFileInput": false
        }),
        "kimi" => json!({
            "streaming": true,
            "temperature": true,
            "topP": true,
            "topK": true,
            "reasoningEffort": false,
            "imageInput": false,
            "textFileInput": true,
            "binaryFileInput": false
        }),
        "qwen" => json!({
            "streaming": true,
            "temperature": true,
            "topP": true,
            "topK": true,
            "reasoningEffort": false,
            "imageInput": false,
            "textFileInput": true,
            "binaryFileInput": false
        }),
        _ => json!({
            "streaming": true,
            "temperature": true,
            "topP": true,
            "topK": !base_url.to_ascii_lowercase().contains("api.openai.com"),
            "reasoningEffort": !reasoning_options.is_empty(),
            "imageInput": false,
            "textFileInput": true,
            "binaryFileInput": false
        }),
    }
}

fn model_options_for_provider(provider_id: &str) -> Vec<String> {
    match provider_id {
        "openai" => vec![
            "gpt-5".to_string(),
            "gpt-5-mini".to_string(),
            "gpt-5-nano".to_string(),
            "gpt-5.1".to_string(),
            "gpt-4.1".to_string(),
        ],
        "anthropic" => vec![
            "claude-opus-4-1-20250805".to_string(),
            "claude-opus-4-20250514".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            "claude-3-7-sonnet-20250219".to_string(),
        ],
        "google-gemini" => vec![
            "gemini-2.5-pro".to_string(),
            "gemini-2.5-flash".to_string(),
            "gemini-2.5-flash-lite".to_string(),
            "gemini-3-flash-preview".to_string(),
        ],
        "openrouter" => vec![
            "openai/gpt-5".to_string(),
            "anthropic/claude-sonnet-4".to_string(),
            "google/gemini-2.5-pro".to_string(),
        ],
        "kimi" => vec![
            "kimi-k2.5".to_string(),
            "kimi-k2-thinking".to_string(),
            "kimi-latest-8k".to_string(),
            "kimi-latest-128k".to_string(),
        ],
        "qwen" => vec![
            "qwen-max".to_string(),
            "qwen-plus".to_string(),
            "qwen-turbo".to_string(),
            "qwen3-max".to_string(),
        ],
        _ => Vec::new(),
    }
}

fn reasoning_options_for_provider(provider_id: &str, model: &str) -> Vec<String> {
    if provider_id != "openai" {
        return Vec::new();
    }
    let model = model.to_ascii_lowercase();
    if model.starts_with("gpt-5") {
        return vec![
            "minimal".to_string(),
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ];
    }
    Vec::new()
}

fn provider_catalog() -> Vec<Value> {
    vec![
        json!({
            "id": "openai",
            "label": "OpenAI",
            "apiStyle": "openai-responses",
            "docsUrl": "https://platform.openai.com/docs/api-reference/responses",
            "authScheme": "bearer",
            "defaultEndpoint": "/responses",
            "baseUrls": [{ "label": "OpenAI Public", "url": "https://api.openai.com/v1", "default": true }],
            "modelDiscovery": "hybrid",
            "sampleModels": ["gpt-5", "gpt-5-mini", "gpt-5-nano", "gpt-5.1", "gpt-4.1"],
            "parameterSupport": {
                "temperature": "supported",
                "topP": "supported",
                "topK": "unsupported",
                "streaming": "supported",
                "imageInput": "supported",
                "textFileInput": "supported",
                "binaryFileInput": "conditional",
                "reasoning": {
                    "mode": "effort",
                    "requestField": "reasoning.effort",
                    "options": ["minimal", "low", "medium", "high", "xhigh"],
                    "notes": [
                        "不同 OpenAI 模型支持的 effort 集合不同，应按模型再裁剪",
                        "topK 未出现在 OpenAI 官方参数文档中，这里按 direct OpenAI 不支持处理"
                    ]
                }
            }
        }),
        json!({
            "id": "anthropic",
            "label": "Anthropic",
            "apiStyle": "anthropic-messages",
            "docsUrl": "https://docs.anthropic.com/en/api/messages",
            "authScheme": "x-api-key",
            "defaultEndpoint": "/v1/messages",
            "baseUrls": [{ "label": "Anthropic Public", "url": "https://api.anthropic.com", "default": true }],
            "modelDiscovery": "static",
            "sampleModels": ["claude-opus-4-1-20250805", "claude-opus-4-20250514", "claude-sonnet-4-20250514", "claude-3-7-sonnet-20250219"],
            "parameterSupport": {
                "temperature": "conditional",
                "topP": "conditional",
                "topK": "conditional",
                "streaming": "supported",
                "imageInput": "supported",
                "textFileInput": "unsupported",
                "binaryFileInput": "unsupported",
                "reasoning": {
                    "mode": "budget_tokens",
                    "requestField": "thinking.budget_tokens",
                    "min": 1024,
                    "notes": [
                        "思考模式通过 thinking 对象开启，而不是 effort 字符串",
                        "启用 thinking 时 temperature 不能设置，top_k 也不兼容，top_p 需固定为 1 或不超过 0.95"
                    ]
                }
            }
        }),
        json!({
            "id": "google-gemini",
            "label": "Google Gemini",
            "apiStyle": "google-gemini",
            "docsUrl": "https://ai.google.dev/gemini-api/docs/text-generation",
            "authScheme": "x-goog-api-key",
            "defaultEndpoint": "/v1beta/models/{model}:generateContent",
            "baseUrls": [{ "label": "Generative Language API", "url": "https://generativelanguage.googleapis.com/v1beta", "default": true }],
            "modelDiscovery": "static",
            "sampleModels": ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-3-flash-preview"],
            "parameterSupport": {
                "temperature": "supported",
                "topP": "supported",
                "topK": "supported",
                "streaming": "supported",
                "imageInput": "supported",
                "textFileInput": "supported",
                "binaryFileInput": "conditional",
                "pdfInput": "supported",
                "reasoning": {
                    "mode": "provider_specific",
                    "notes": [
                        "Gemini 2.5 系列用 thinkingBudget",
                        "Gemini 3 系列用 thinkingConfig.thinkingLevel",
                        "是否可完全关闭思考取决于具体模型，例如 2.5 Flash 可设为 0，2.5 Pro 不能关闭"
                    ]
                }
            }
        }),
        json!({
            "id": "openrouter",
            "label": "OpenRouter",
            "apiStyle": "openrouter-chat",
            "docsUrl": "https://openrouter.ai/docs/api-reference/overview",
            "authScheme": "bearer",
            "defaultEndpoint": "/chat/completions",
            "baseUrls": [{ "label": "OpenRouter", "url": "https://openrouter.ai/api/v1", "default": true }],
            "modelDiscovery": "hybrid",
            "modelListEndpoint": "https://openrouter.ai/api/v1/models",
            "sampleModels": ["openai/gpt-5", "anthropic/claude-sonnet-4", "google/gemini-2.5-pro"],
            "parameterSupport": {
                "temperature": "conditional",
                "topP": "conditional",
                "topK": "conditional",
                "streaming": "supported",
                "imageInput": "conditional",
                "textFileInput": "conditional",
                "binaryFileInput": "conditional",
                "reasoning": {
                    "mode": "model_metadata",
                    "notes": [
                        "OpenRouter 不适合下发固定 reasoningOptions，应以 /models 返回的 supported_parameters 为准",
                        "不同上游模型参数支持范围不同"
                    ]
                }
            }
        }),
        json!({
            "id": "kimi",
            "label": "Kimi API",
            "apiStyle": "openai-chat",
            "docsUrl": "https://platform.moonshot.ai/docs/guide/use-kimi-k2-thinking-model.en-US",
            "authScheme": "bearer",
            "defaultEndpoint": "/chat/completions",
            "baseUrls": [{ "label": "Moonshot Public", "url": "https://api.moonshot.ai/v1", "default": true }],
            "modelDiscovery": "hybrid",
            "sampleModels": ["kimi-k2.5", "kimi-k2-thinking", "kimi-latest-8k", "kimi-latest-128k"],
            "parameterSupport": {
                "temperature": "conditional",
                "topP": "conditional",
                "topK": "conditional",
                "streaming": "supported",
                "imageInput": "supported",
                "textFileInput": "supported",
                "binaryFileInput": "conditional",
                "reasoning": {
                    "mode": "enabled_disabled_or_dedicated_model",
                    "notes": [
                        "kimi-k2.5 默认开启 thinking capability，也可切换 disabled",
                        "kimi-k2-thinking 是强制思考模型",
                        "K2.5 模型的 temperature 与 top_p 为固定值，需后端按模型禁用对应滑杆"
                    ]
                }
            }
        }),
        json!({
            "id": "qwen",
            "label": "Qwen API",
            "apiStyle": "openai-chat",
            "docsUrl": "https://www.alibabacloud.com/help/en/model-studio/use-qwen-by-calling-api",
            "authScheme": "bearer",
            "defaultEndpoint": "/compatible-mode/v1/chat/completions",
            "baseUrls": [
                { "label": "Beijing", "url": "https://dashscope.aliyuncs.com/compatible-mode/v1", "default": true },
                { "label": "Singapore", "url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", "region": "ap-southeast-1" },
                { "label": "US Virginia", "url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", "region": "us-east-1" }
            ],
            "modelDiscovery": "hybrid",
            "sampleModels": ["qwen-max", "qwen-plus", "qwen-turbo", "qwen3-max"],
            "parameterSupport": {
                "temperature": "conditional",
                "topP": "conditional",
                "topK": "unknown",
                "streaming": "supported",
                "imageInput": "conditional",
                "textFileInput": "conditional",
                "binaryFileInput": "conditional",
                "reasoning": {
                    "mode": "enable_thinking_and_budget",
                    "requestField": "extra_body.enable_thinking",
                    "notes": [
                        "Qwen OpenAI 兼容模式通过 extra_body.enable_thinking 控制深度思考",
                        "Qwen3-Max 等模型还支持 thinking_budget"
                    ]
                }
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_reasoning_options_follow_model_family() {
        assert_eq!(
            reasoning_options_for_provider("openai", "gpt-5-mini"),
            vec![
                "minimal".to_string(),
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
            ]
        );
        assert!(reasoning_options_for_provider("openai", "gpt-4.1").is_empty());
    }

    #[test]
    fn responses_input_keeps_image_attachments_for_openai() {
        let request = WebLlmChatRequest {
            message: "describe this".to_string(),
            attachments: vec![WebLlmAttachment {
                kind: "image".to_string(),
                name: "clipboard.png".to_string(),
                media_type: Some("image/png".to_string()),
                size: Some(3),
                data_url: Some("data:image/png;base64,abc".to_string()),
                text: None,
            }],
            ..Default::default()
        };

        let input =
            build_responses_input(&request, "be concise", "openai").expect("input should build");
        let content = input[0]["content"]
            .as_array()
            .expect("responses input content should be an array");
        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "data:image/png;base64,abc");
    }

    #[test]
    fn anthropic_request_embeds_base64_image_blocks() {
        let request = WebLlmChatRequest {
            messages: vec![WebLlmMessage {
                role: "user".to_string(),
                content: "look".to_string(),
                attachments: vec![WebLlmAttachment {
                    kind: "image".to_string(),
                    name: "clipboard.png".to_string(),
                    media_type: Some("image/png".to_string()),
                    size: None,
                    data_url: Some("data:image/png;base64,abcd".to_string()),
                    text: None,
                }],
            }],
            ..Default::default()
        };

        let payload = build_anthropic_request(
            &request,
            Some("system"),
            "persona",
            "claude-sonnet-4-20250514",
            Some(0.7),
            Some(0.95),
            Some(32),
            Some("medium"),
        )
        .expect("anthropic payload should build");

        assert_eq!(
            payload["system"],
            "system\n\nPrompt profile instruction:\npersona"
        );
        assert_eq!(
            payload["messages"][0]["content"][1]["source"]["type"],
            "base64"
        );
        assert_eq!(
            payload["messages"][0]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(
            payload["messages"][0]["content"][1]["source"]["data"],
            "abcd"
        );
        assert_eq!(payload["thinking"]["budget_tokens"], 4096);
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("top_k").is_none());
    }

    #[test]
    fn gemini_request_uses_inline_data_parts() {
        let request = WebLlmChatRequest {
            messages: vec![WebLlmMessage {
                role: "user".to_string(),
                content: "".to_string(),
                attachments: vec![WebLlmAttachment {
                    kind: "image".to_string(),
                    name: "clipboard.png".to_string(),
                    media_type: Some("image/jpeg".to_string()),
                    size: None,
                    data_url: Some("data:image/jpeg;base64,xyz".to_string()),
                    text: None,
                }],
            }],
            ..Default::default()
        };

        let payload = build_gemini_request(
            &request,
            None,
            "",
            "gemini-2.5-flash",
            Some(0.3),
            Some(0.8),
            Some(20),
            Some("minimal"),
        )
        .expect("gemini payload should build");

        assert_eq!(
            payload["contents"][0]["parts"][0]["inline_data"]["mime_type"],
            "image/jpeg"
        );
        assert_eq!(
            payload["contents"][0]["parts"][0]["inline_data"]["data"],
            "xyz"
        );
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );
    }

    #[test]
    fn openrouter_request_uses_multimodal_messages() {
        let request = WebLlmChatRequest {
            messages: vec![WebLlmMessage {
                role: "user".to_string(),
                content: "describe".to_string(),
                attachments: vec![WebLlmAttachment {
                    kind: "image".to_string(),
                    name: "clipboard.png".to_string(),
                    media_type: Some("image/png".to_string()),
                    size: None,
                    data_url: Some("data:image/png;base64,abc".to_string()),
                    text: None,
                }],
            }],
            ..Default::default()
        };

        let payload = build_openrouter_request(
            &request,
            None,
            "persona",
            "openai/gpt-5",
            Some(0.5),
            Some(0.9),
            Some("high"),
        )
        .expect("openrouter payload should build");

        assert_eq!(payload["messages"][0]["role"], "system");
        assert_eq!(payload["messages"][1]["content"][0]["type"], "text");
        assert_eq!(payload["messages"][1]["content"][1]["type"], "image_url");
        assert_eq!(
            payload["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,abc"
        );
        assert_eq!(payload["reasoning"]["effort"], "high");
    }

    #[test]
    fn frontend_chat_summary_avoids_prompt_content_and_counts_payload() {
        let request = WebLlmChatRequest {
            messages: vec![
                WebLlmMessage {
                    role: "system".to_string(),
                    content: "hidden system prompt".to_string(),
                    attachments: vec![],
                },
                WebLlmMessage {
                    role: "user".to_string(),
                    content: "hello from frontend".to_string(),
                    attachments: vec![WebLlmAttachment {
                        kind: "image".to_string(),
                        name: "clipboard.png".to_string(),
                        media_type: Some("image/png".to_string()),
                        size: Some(12),
                        data_url: Some("data:image/png;base64,abc".to_string()),
                        text: None,
                    }],
                },
            ],
            ..Default::default()
        };

        let summary = summarize_frontend_chat_request(
            &request,
            "openai",
            "https://api.openai.com/v1",
            "gpt-5-mini",
            "default",
            Some("medium"),
        );

        assert!(summary.contains("provider=openai"));
        assert!(summary.contains("turns=2"));
        assert!(summary.contains("attachments=1"));
        assert!(summary.contains("input_chars=39"));
        assert!(summary.contains("reasoning=medium"));
        assert!(!summary.contains("hidden system prompt"));
        assert!(!summary.contains("hello from frontend"));
    }

    #[test]
    fn fallback_prompt_keeps_multi_turn_history_in_transcript() {
        let request = WebLlmChatRequest {
            messages: vec![
                WebLlmMessage {
                    role: "user".to_string(),
                    content: "你好".to_string(),
                    attachments: vec![],
                },
                WebLlmMessage {
                    role: "assistant".to_string(),
                    content: "你好！有什么我可以帮助你的吗？".to_string(),
                    attachments: vec![],
                },
                WebLlmMessage {
                    role: "user".to_string(),
                    content: "0.9的9循环和1是否相等".to_string(),
                    attachments: vec![],
                },
            ],
            ..Default::default()
        };

        let prompt =
            compose_chat_fallback_prompt(&request, "只说一句结论").expect("prompt should build");

        assert!(prompt.contains("Prompt profile instruction:\n只说一句结论"));
        assert!(prompt.contains("user:\n你好"));
        assert!(prompt.contains("assistant:\n你好！有什么我可以帮助你的吗？"));
        assert!(prompt.contains("user:\n0.9的9循环和1是否相等"));
    }

    #[test]
    fn preview_payload_redacts_prompt_like_text_fields() {
        let payload = json!({
            "instructions": "secret system prompt",
            "input": [
                {
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "secret composed prompt" }
                    ]
                }
            ],
            "model": "gpt-5"
        });

        let redacted = redact_preview_payload(&payload);

        assert_eq!(redacted["instructions"], "<redacted>");
        assert_eq!(redacted["input"][0]["content"][0]["text"], "<redacted>");
        assert_eq!(redacted["model"], "gpt-5");
    }

    #[test]
    fn normalize_single_provider_input_keeps_explicit_provider_and_trims_headers() {
        let provider = WebLlmManagedProviderPayload {
            id: " Gemini Main ".to_string(),
            label: " Gemini Main ".to_string(),
            provider_id: Some("google-gemini".to_string()),
            base_url: " https://generativelanguage.googleapis.com/v1beta ".to_string(),
            api_key: " secret ".to_string(),
            timeout_seconds: Some(0),
            headers: HashMap::from([
                (" X-Test ".to_string(), " ok ".to_string()),
                (" ".to_string(), "ignored".to_string()),
            ]),
            models: vec![WebLlmManagedModelPayload {
                id: " gemini-2.5-pro ".to_string(),
                enabled: true,
            }],
        };

        let normalized = normalize_single_provider_input(&provider, 0, 120);

        assert_eq!(normalized.id.as_deref(), Some("gemini-main"));
        assert_eq!(normalized.label.as_deref(), Some("Gemini Main"));
        assert_eq!(normalized.provider.as_deref(), Some("google-gemini"));
        assert_eq!(
            normalized.base_url.as_deref(),
            Some("https://generativelanguage.googleapis.com/v1beta")
        );
        assert_eq!(normalized.api_key.as_deref(), Some("secret"));
        assert_eq!(normalized.timeout_seconds, Some(1));
        assert_eq!(
            normalized
                .headers
                .as_ref()
                .and_then(|headers| headers.get("X-Test"))
                .map(String::as_str),
            Some("ok")
        );
        assert_eq!(
            normalized
                .models
                .as_ref()
                .and_then(|models| models.first())
                .and_then(|model| model.id.as_deref()),
            Some("gemini-2.5-pro")
        );
    }

    #[test]
    fn discover_models_for_provider_reports_catalog_when_remote_lookup_falls_back() {
        let provider = LlmManagedProviderConfig {
            provider: Some("openai".to_string()),
            base_url: Some(String::new()),
            ..Default::default()
        };

        let (models, source) =
            discover_models_for_provider(&provider).expect("fallback discovery should succeed");

        assert_eq!(source, "catalog");
        assert!(models.iter().any(|model| model == "gpt-5"));
    }

    #[test]
    fn reject_non_post_method_returns_method_error_response() {
        let response =
            reject_non_post_method("GET", "LLM/SaveManagerState", false).expect("response");
        let text = String::from_utf8(response).expect("response should be utf8");
        assert!(text.contains("LLM/SaveManagerState only accepts POST"));
        assert!(reject_non_post_method("POST", "LLM/SaveManagerState", false).is_none());
    }
}
