use std::collections::HashMap;

use serde_json::Value;

use super::super::ProviderAuth;

pub(in super::super) fn preview_headers_map(
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
        headers.insert(
            name.clone(),
            redact_preview_header_value(name.as_str(), value.as_str()),
        );
    }
    headers
}

pub(in super::super) fn redact_preview_payload(payload: &Value) -> Value {
    match payload {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let redacted = match key.as_str() {
                        "instructions" | "system" | "text" => redacted_text_value(),
                        "content" => redact_preview_content(value),
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

fn redact_preview_content(value: &Value) -> Value {
    match value {
        Value::String(_) => redacted_text_value(),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| match item {
                    Value::String(_) => redacted_text_value(),
                    _ => redact_preview_payload(item),
                })
                .collect(),
        ),
        _ => redact_preview_payload(value),
    }
}

fn redacted_text_value() -> Value {
    Value::String("<redacted>".to_string())
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

fn redact_preview_header_value(name: &str, value: &str) -> String {
    if !is_sensitive_header_name(name) {
        return value.to_string();
    }

    if name.trim().eq_ignore_ascii_case("authorization")
        || name.trim().eq_ignore_ascii_case("proxy-authorization")
    {
        let trimmed = value.trim();
        if let Some((scheme, credentials)) = trimmed.split_once(' ')
            && !scheme.trim().is_empty()
            && !credentials.trim().is_empty()
        {
            return format!("{} {}", scheme.trim(), mask_secret(credentials));
        }
    }

    mask_secret(value)
}

fn is_sensitive_header_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower == "authorization"
        || lower == "proxy-authorization"
        || lower == "cookie"
        || lower == "set-cookie"
        || lower.contains("api-key")
        || lower.contains("api_key")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("passwd")
        || lower.contains("password")
}
