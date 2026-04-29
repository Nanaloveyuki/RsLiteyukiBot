use serde_json::{Map, Value, json};

use super::super::types::{WebLlmChatRequest, WebLlmMessage, WebLlmRuntimeConfig};
use super::shared::{
    compose_system_instruction, effective_messages, non_empty_attachment_name,
    normalize_message_role,
};

// 外部调用
#[allow(dead_code)]
pub(in super::super) fn build_openrouter_request(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
    model: &str,
    temperature: Option<f32>,
    top_p: Option<f32>,
    frequency_penalty: Option<f32>,
    presence_penalty: Option<f32>,
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
    if let Some(frequency_penalty) = frequency_penalty {
        body.insert("frequency_penalty".to_string(), json!(frequency_penalty));
    }
    if let Some(presence_penalty) = presence_penalty {
        body.insert("presence_penalty".to_string(), json!(presence_penalty));
    }
    if let Some(reasoning_effort) = reasoning_effort {
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": reasoning_effort }),
        );
    }

    Ok(Value::Object(body))
}

pub(in super::super) fn build_chat_completions_messages(
    request: &WebLlmChatRequest,
    runtime_system_prompt: Option<&str>,
    soul: &str,
) -> Result<Vec<Value>, String> {
    build_openrouter_messages(request, runtime_system_prompt, soul)
}

pub(in super::super) fn build_chat_completions_request_payload(
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
        && message.content.trim() == text
        && message.attachments.is_empty()
        && parts[0].get("type").and_then(Value::as_str) == Some("text")
    {
        Ok(Some(Value::String(text.to_string())))
    } else {
        Ok(Some(Value::Array(parts)))
    }
}
