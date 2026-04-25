use std::path::PathBuf;
use std::time::Duration;

use reqwest::Client;
use serde_json::{Map, Value, json};

use crate::app_config::{AppConfigDoc, LlmRuntimeConfig, resolve_llm_config};
use crate::config_paths::resolve_preferred_llm_prompt_store_path;
use crate::i18n::{tr, trf};
use crate::llm::tools::{ToolManager, merge_system_prompt_sections};
use crate::llm::{
    LlmClientError, LlmCompletion, LlmEventSink, LlmFunctionTool, LlmPromptProfile, LlmPromptStore,
    OpenAiResponsesClient, compose_user_prompt,
};
use crate::runtime_support::{load_app_config_with_llm_overlay, next_llm_api_key_index};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LlmProviderApiFamily {
    Responses,
    ChatCompletions,
    AnthropicMessages,
    GeminiGenerateContent,
}

pub(crate) async fn generate_llm_reply(prompt: &str) -> Result<String, String> {
    complete_llm_prompt(prompt, &[], None)
        .await
        .map(|completion| completion.text)
}

#[allow(dead_code)]
pub(crate) async fn generate_llm_reply_with_events(
    prompt: &str,
    sink: &mut dyn LlmEventSink,
) -> Result<LlmCompletion, String> {
    complete_llm_prompt(prompt, &[], Some(sink)).await
}

#[allow(dead_code)]
pub(crate) async fn complete_llm_prompt(
    prompt: &str,
    tools: &[LlmFunctionTool],
    sink: Option<&mut dyn LlmEventSink>,
) -> Result<LlmCompletion, String> {
    let llm_config = current_llm_runtime_config()?;
    if !llm_config.enabled {
        return Err(tr("main.llm.disabled"));
    }
    let Some(api_key) = pick_next_api_key(&llm_config) else {
        return Err(tr("main.llm.api_key_missing"));
    };
    let prompt_profile = current_active_prompt_profile()?;
    let composed_prompt = compose_user_prompt(prompt, prompt_profile.soul.as_str());
    let provider_id = resolve_runtime_provider_id(&llm_config);
    let provider_family = provider_api_family(provider_id.as_str());

    match provider_family {
        LlmProviderApiFamily::Responses | LlmProviderApiFamily::ChatCompletions => {
            let mut llm_config = llm_config;
            let capability_bundle = ToolManager::for_current_workspace()?
                .build_runtime_bundle(tools)
                .await?;
            llm_config.system_prompt = merge_system_prompt_sections(
                llm_config.system_prompt.as_deref(),
                [capability_bundle.system_prompt.as_deref()],
            );

            let client = OpenAiResponsesClient::from_runtime_with_api_key(&llm_config, &api_key)
                .map_err(|err: LlmClientError| err.to_string())?;
            match provider_family {
                LlmProviderApiFamily::Responses => client
                    .complete(
                        composed_prompt.as_str(),
                        capability_bundle.tools.as_slice(),
                        sink,
                    )
                    .await
                    .map_err(|err| err.to_string()),
                LlmProviderApiFamily::ChatCompletions => client
                    .complete_with_chat_completions(
                        composed_prompt.as_str(),
                        capability_bundle.tools.as_slice(),
                        sink,
                    )
                    .await
                    .map_err(|err| err.to_string()),
                _ => unreachable!("provider family already matched above"),
            }
        }
        LlmProviderApiFamily::AnthropicMessages | LlmProviderApiFamily::GeminiGenerateContent => {
            let text = complete_prompt_without_tool_runtime(
                composed_prompt.as_str(),
                &llm_config,
                &api_key,
                provider_id.as_str(),
            )
            .await?;
            Ok(LlmCompletion {
                text,
                tool_calls: Vec::new(),
            })
        }
    }
}

#[allow(dead_code)]
pub(crate) async fn probe_llm_runtime_text(
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
    prompt: &str,
) -> Result<String, String> {
    let provider_id = resolve_runtime_provider_id(llm_config);
    match provider_api_family(provider_id.as_str()) {
        LlmProviderApiFamily::Responses => {
            let client = OpenAiResponsesClient::from_runtime_with_api_key(llm_config, api_key)
                .map_err(|err| err.to_string())?;
            client.generate(prompt).await.map_err(|err| err.to_string())
        }
        LlmProviderApiFamily::ChatCompletions => {
            let client = OpenAiResponsesClient::from_runtime_with_api_key(llm_config, api_key)
                .map_err(|err| err.to_string())?;
            client
                .generate_with_chat_completions(prompt)
                .await
                .map_err(|err| err.to_string())
        }
        LlmProviderApiFamily::AnthropicMessages | LlmProviderApiFamily::GeminiGenerateContent => {
            complete_prompt_without_tool_runtime(prompt, llm_config, api_key, provider_id.as_str())
                .await
        }
    }
}

pub(crate) fn resolve_runtime_provider_id(llm_config: &LlmRuntimeConfig) -> String {
    resolve_provider_id(
        Some(llm_config.provider.as_str()),
        Some(llm_config.base_url.as_str()),
    )
}

#[allow(dead_code)]
pub(crate) fn provider_uses_openai_runtime(provider_id: &str) -> bool {
    matches!(
        provider_api_family(provider_id),
        LlmProviderApiFamily::Responses | LlmProviderApiFamily::ChatCompletions
    )
}

pub(crate) fn provider_api_family(provider_id: &str) -> LlmProviderApiFamily {
    match provider_id {
        "openai" | "openai-compatible" => LlmProviderApiFamily::Responses,
        "openrouter" | "kimi" | "qwen" => LlmProviderApiFamily::ChatCompletions,
        "anthropic" => LlmProviderApiFamily::AnthropicMessages,
        "google-gemini" => LlmProviderApiFamily::GeminiGenerateContent,
        _ => LlmProviderApiFamily::Responses,
    }
}

pub(crate) fn resolve_provider_id(
    configured_provider: Option<&str>,
    base_url: Option<&str>,
) -> String {
    let configured = configured_provider.and_then(normalize_provider_id);
    let detected = base_url
        .map(detect_provider_id_from_base_url)
        .unwrap_or_else(|| "openai-compatible".to_string());

    match configured.as_deref() {
        Some("openai") => match detected.as_str() {
            "openrouter" | "kimi" | "qwen" => detected,
            _ => "openai".to_string(),
        },
        Some("openai-compatible") => match detected.as_str() {
            "openai" => "openai-compatible".to_string(),
            _ => detected,
        },
        Some(explicit @ ("openrouter" | "kimi" | "qwen" | "anthropic" | "google-gemini")) => {
            explicit.to_string()
        }
        Some(_) | None => detected,
    }
}

pub(crate) fn detect_provider_id_from_base_url(base_url: &str) -> String {
    let normalized = base_url.to_ascii_lowercase();
    if normalized.contains("api.openai.com") {
        "openai".to_string()
    } else if normalized.contains("openrouter.ai") {
        "openrouter".to_string()
    } else if normalized.contains("moonshot.ai") {
        "kimi".to_string()
    } else if normalized.contains("dashscope.aliyuncs.com") {
        "qwen".to_string()
    } else if normalized.contains("generativelanguage.googleapis.com") {
        "google-gemini".to_string()
    } else if normalized.contains("anthropic.com") {
        "anthropic".to_string()
    } else {
        "openai-compatible".to_string()
    }
}

fn normalize_provider_id(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" => None,
        "openai" | "openai-compatible" | "openrouter" | "kimi" | "qwen" | "anthropic"
        | "google-gemini" => Some(normalized),
        _ => None,
    }
}

async fn complete_prompt_without_tool_runtime(
    prompt: &str,
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
    provider_id: &str,
) -> Result<String, String> {
    match provider_id {
        "anthropic" => complete_anthropic_prompt(prompt, llm_config, api_key).await,
        "google-gemini" => complete_gemini_prompt(prompt, llm_config, api_key).await,
        _ => Err(trf(
            "main.llm.provider.unsupported",
            &[("provider", provider_id)],
        )),
    }
}

async fn complete_anthropic_prompt(
    prompt: &str,
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
) -> Result<String, String> {
    let payload = build_anthropic_prompt_request(
        llm_config.model.as_str(),
        prompt,
        llm_config.system_prompt.as_deref(),
        llm_config.temperature,
        llm_config.top_p,
        llm_config.top_k,
    );
    let response = send_provider_json_request(
        llm_config,
        join_api_endpoint(llm_config.base_url.as_str(), "v1/messages").as_str(),
        ProviderAuth::ApiKeyHeader("x-api-key"),
        &[("anthropic-version", "2023-06-01")],
        &payload,
        api_key,
    )
    .await?;
    extract_anthropic_text(&response)
        .ok_or_else(|| "Anthropic response did not contain assistant text".to_string())
}

async fn complete_gemini_prompt(
    prompt: &str,
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
) -> Result<String, String> {
    let payload = build_gemini_prompt_request(
        llm_config.model.as_str(),
        prompt,
        llm_config.system_prompt.as_deref(),
        llm_config.temperature,
        llm_config.top_p,
        llm_config.top_k,
    );
    let response = send_provider_json_request(
        llm_config,
        join_api_endpoint(
            llm_config.base_url.as_str(),
            format!("models/{}:generateContent", llm_config.model).as_str(),
        )
        .as_str(),
        ProviderAuth::ApiKeyHeader("x-goog-api-key"),
        &[],
        &payload,
        api_key,
    )
    .await?;
    extract_gemini_text(&response)
        .ok_or_else(|| "Gemini response did not contain assistant text".to_string())
}

async fn send_provider_json_request(
    llm_config: &LlmRuntimeConfig,
    endpoint: &str,
    auth: ProviderAuth<'_>,
    extra_headers: &[(&str, &str)],
    payload: &Value,
    api_key: &str,
) -> Result<Value, String> {
    let client = build_provider_http_client(llm_config.timeout_ms)?;
    let mut request = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json");

    request = match auth {
        ProviderAuth::ApiKeyHeader(header) => request.header(header, api_key),
    };

    for (name, value) in &llm_config.headers {
        request = request.header(name, value);
    }
    for (name, value) in extra_headers {
        request = request.header(*name, *value);
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
        return Err(format!(
            "upstream returned {}: {}",
            status.as_u16(),
            body.trim()
        ));
    }
    serde_json::from_str(body.as_str())
        .map_err(|err| format!("upstream returned invalid JSON: {err}"))
}

fn build_provider_http_client(timeout_ms: u64) -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(250)))
        .user_agent("RsLiteyukiBot-Service/0.1")
        .build()
        .map_err(|err| format!("failed to build LLM http client: {err}"))
}

fn build_anthropic_prompt_request(
    model: &str,
    prompt: &str,
    system_prompt: Option<&str>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
) -> Value {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert("max_tokens".to_string(), json!(2048));
    body.insert(
        "messages".to_string(),
        json!([
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": prompt,
                    }
                ],
            }
        ]),
    );
    if let Some(system_prompt) = system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "system".to_string(),
            Value::String(system_prompt.to_string()),
        );
    }
    if let Some(temperature) = temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(top_k) = top_k {
        body.insert("top_k".to_string(), json!(top_k));
    }
    Value::Object(body)
}

fn build_gemini_prompt_request(
    model: &str,
    prompt: &str,
    system_prompt: Option<&str>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
) -> Value {
    let mut body = Map::new();
    let _ = model;
    body.insert(
        "contents".to_string(),
        json!([
            {
                "role": "user",
                "parts": [
                    {
                        "text": prompt,
                    }
                ],
            }
        ]),
    );
    if let Some(system_prompt) = system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "system_instruction".to_string(),
            json!({
                "parts": [{ "text": system_prompt }],
            }),
        );
    }
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
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }
    Value::Object(body)
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

fn join_api_endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim().trim_end_matches('/'),
        path.trim().trim_start_matches('/')
    )
}

enum ProviderAuth<'a> {
    ApiKeyHeader(&'a str),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_config(provider: &str, base_url: &str) -> LlmRuntimeConfig {
        LlmRuntimeConfig {
            enabled: true,
            stream: false,
            provider: provider.to_string(),
            base_url: base_url.to_string(),
            api_keys: vec!["sk-test".to_string()],
            headers: Default::default(),
            model: "gpt-5-mini".to_string(),
            timeout_ms: 20_000,
            temperature: None,
            top_p: None,
            top_k: None,
            parallel_tool_calls: true,
            system_prompt: None,
            command_prefix: "/ask".to_string(),
        }
    }

    #[test]
    fn resolve_runtime_provider_id_prefers_detected_openai_family_from_base_url() {
        let config = runtime_config("openai", "https://openrouter.ai/api");
        assert_eq!(resolve_runtime_provider_id(&config), "openrouter");
    }

    #[test]
    fn provider_uses_openai_runtime_accepts_openai_style_providers() {
        assert!(provider_uses_openai_runtime("openai"));
        assert!(provider_uses_openai_runtime("openai-compatible"));
        assert!(provider_uses_openai_runtime("qwen"));
        assert!(!provider_uses_openai_runtime("anthropic"));
        assert!(!provider_uses_openai_runtime("google-gemini"));
    }

    #[test]
    fn resolve_provider_id_prefers_explicit_provider_for_custom_gateway() {
        assert_eq!(
            resolve_provider_id(
                Some("anthropic"),
                Some("https://llm-proxy.internal/company-gateway"),
            ),
            "anthropic"
        );
        assert_eq!(
            resolve_provider_id(
                Some("google-gemini"),
                Some("https://gateway.example/v1beta"),
            ),
            "google-gemini"
        );
    }
}

pub(crate) fn current_llm_runtime_config() -> Result<LlmRuntimeConfig, String> {
    let doc = load_current_app_config_doc()?;
    Ok(resolve_llm_config(&doc))
}

pub(crate) fn load_current_app_config_doc() -> Result<AppConfigDoc, String> {
    let (doc, _) = load_app_config_with_llm_overlay();
    Ok(doc)
}

pub(crate) fn current_active_prompt_profile() -> Result<LlmPromptProfile, String> {
    let store = load_llm_prompt_store()?;
    Ok(store.active_profile())
}

pub(crate) fn load_llm_prompt_store() -> Result<LlmPromptStore, String> {
    let path = resolve_llm_prompt_store_path();
    LlmPromptStore::load_or_default_from_path(path.as_path())
}

#[allow(dead_code)]
pub(crate) fn persist_llm_prompt_store(store: &LlmPromptStore) -> Result<PathBuf, String> {
    let path = resolve_llm_prompt_store_path();
    store.save_to_path(path.as_path())?;
    Ok(path)
}

pub(crate) fn resolve_llm_prompt_store_path() -> PathBuf {
    resolve_preferred_llm_prompt_store_path()
}

fn pick_next_api_key(llm_config: &LlmRuntimeConfig) -> Option<String> {
    let index = next_llm_api_key_index(llm_config.api_keys.len())?;
    llm_config.api_keys.get(index).cloned()
}
