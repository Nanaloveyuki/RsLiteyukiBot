use std::time::Duration;

use reqwest::Client;
use serde_json::{Map, Value, json};

use crate::app_config::LlmRuntimeConfig;
use crate::i18n::trf;

pub(super) async fn complete_prompt_without_tool_runtime(
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
