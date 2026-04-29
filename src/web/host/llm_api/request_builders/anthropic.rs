use serde_json::{Map, Value, json};

use super::super::types::{WebLlmChatRequest, WebLlmMessage};
use super::shared::{
    compose_system_instruction, effective_messages, non_empty_attachment_name,
    normalize_message_role, parse_data_url,
};

const DEFAULT_WEB_LLM_MAX_OUTPUT_TOKENS: u32 = 2048;

pub(in super::super) fn build_anthropic_request(
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

pub(in super::super) fn extract_anthropic_text(payload: &Value) -> Option<String> {
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
