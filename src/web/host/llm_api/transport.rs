use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use super::{ProviderAuth, truncate_inline};

use crate::LogLevel;
use crate::emit_console_log;

pub(super) fn collect_runtime_headers(
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

pub(super) async fn send_json_request(
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

pub(super) async fn send_json_request_with_optional_auth(
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

pub(super) async fn send_json_get_request(
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

fn build_provider_http_client(timeout_ms: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(250)))
        .user_agent("RsLiteyukiBot-WebHost/0.1")
        .build()
        .map_err(|err| format!("failed to build LLM http client: {err}"))
}

pub(super) fn summarize_llm_upstream_error(status: reqwest::StatusCode, body: &str) -> String {
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

pub(super) fn join_api_endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim().trim_end_matches('/'),
        path.trim().trim_start_matches('/')
    )
}
