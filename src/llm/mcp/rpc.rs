use liteyukibot_core::SseParser;
use serde_json::Value;

use crate::llm::client::LlmToolOutput;

pub(super) struct McpHttpResponse {
    pub(super) payload: Option<Value>,
    pub(super) session_id: Option<String>,
}

pub(super) fn parse_sse_json_payload(body: &str) -> Result<Value, String> {
    let mut parser = SseParser::default();
    let events = parser.push_chunk(body);
    let mut fallback_payload = None;
    for event in events {
        let data = event.data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(payload) = serde_json::from_str::<Value>(data) {
            if payload.get("result").is_some() || payload.get("error").is_some() {
                return Ok(payload);
            }
            fallback_payload = Some(payload);
        }
    }
    if let Some(payload) = fallback_payload
        && payload.is_object()
    {
        return Ok(payload);
    }
    Err("MCP SSE response did not contain a JSON payload".to_string())
}

pub(super) fn extract_json_rpc_result(payload: Option<&Value>) -> Result<&Value, String> {
    let payload =
        payload.ok_or_else(|| "MCP response did not contain a JSON payload".to_string())?;
    if let Some(error) = payload.get("error") {
        return Err(format!("MCP JSON-RPC error: {error}"));
    }
    payload
        .get("result")
        .ok_or_else(|| "MCP JSON-RPC response missing result".to_string())
}

pub(super) fn parse_call_tool_output(payload: Option<&Value>) -> Result<LlmToolOutput, String> {
    let result = extract_json_rpc_result(payload)?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(format!(
            "remote MCP tool returned isError=true: {}",
            render_call_result_text(result).unwrap_or_else(|| result.to_string())
        ));
    }

    if result.get("structuredContent").is_some() {
        return Ok(LlmToolOutput::Json(result.clone()));
    }
    if let Some(text) = render_call_result_text(result) {
        return Ok(LlmToolOutput::Text(text));
    }
    Ok(LlmToolOutput::Json(result.clone()))
}

fn render_call_result_text(result: &Value) -> Option<String> {
    let mut fragments = Vec::new();
    for item in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
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
