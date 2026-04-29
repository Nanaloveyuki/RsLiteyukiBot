use super::super::types::{WebLlmAttachment, WebLlmChatRequest, WebLlmMessage};

#[derive(Debug, Clone)]
pub(super) struct ParsedDataUrl {
    pub(super) media_type: String,
    pub(super) data: String,
}

pub(in super::super) fn compose_chat_fallback_prompt(
    request: &WebLlmChatRequest,
    soul: &str,
) -> Result<String, String> {
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

pub(in super::super) fn compose_system_instruction(
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

pub(in super::super) fn effective_messages(request: &WebLlmChatRequest) -> Vec<WebLlmMessage> {
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

pub(super) fn latest_turn_attachments(request: &WebLlmChatRequest) -> &[WebLlmAttachment] {
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

pub(super) fn attachment_summary_lines(
    attachments: &[WebLlmAttachment],
) -> Result<Vec<String>, String> {
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

pub(in super::super) fn truncate_inline(raw: &str, limit: usize) -> String {
    let mut chars = raw.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

pub(super) fn parse_data_url(
    raw: &str,
    fallback_media_type: Option<&str>,
) -> Result<ParsedDataUrl, String> {
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

pub(super) fn non_empty_attachment_name(attachment: &WebLlmAttachment) -> String {
    let name = attachment.name.trim();
    if name.is_empty() {
        "unnamed".to_string()
    } else {
        name.to_string()
    }
}

pub(super) fn normalize_message_role(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "assistant" => "assistant",
        "system" => "system",
        _ => "user",
    }
}
