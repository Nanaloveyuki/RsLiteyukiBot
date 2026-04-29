use serde_json::{Value, json};

use super::super::types::{WebLlmChatRequest, WebLlmRuntimeConfig};
use super::shared::{
    compose_chat_fallback_prompt, latest_turn_attachments, non_empty_attachment_name,
};

pub(in super::super) fn build_responses_input(
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

pub(in super::super) fn build_openai_compatible_request_payload(
    request: &WebLlmChatRequest,
    runtime: &WebLlmRuntimeConfig,
    soul: &str,
    provider_id: &str,
) -> Result<Value, String> {
    let mut body = serde_json::Map::new();
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
