use serde_json::{Map, Value, json};

use super::super::types::{WebLlmChatRequest, WebLlmMessage};
use super::shared::{
    compose_system_instruction, effective_messages, non_empty_attachment_name,
    normalize_message_role, parse_data_url,
};

pub(in super::super) fn build_gemini_request(
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

pub(in super::super) fn extract_gemini_text(payload: &Value) -> Option<String> {
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
