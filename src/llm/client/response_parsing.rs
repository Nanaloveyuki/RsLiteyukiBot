use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::event_dispatch::EventDispatcher;
use super::protocol::{ChatMessage, PendingToolCall, ProviderTurn, ResponsesToolCallState};
use super::{LlmClientError, LlmStreamEvent, OUTPUT_TRUNCATE_LIMIT};

pub(super) fn append_turn_text(output: &mut String, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if output.is_empty() {
        output.push_str(trimmed);
    } else {
        output.push('\n');
        output.push_str(trimmed);
    }
}

#[derive(Default)]
pub(super) struct ResponsesStreamState {
    completed_response: Option<Value>,
    tool_states: HashMap<String, ResponsesToolCallState>,
    emitted_tool_calls: HashSet<String>,
}

impl ResponsesStreamState {
    pub(super) fn apply_event(
        &mut self,
        payload: &Value,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<(), LlmClientError> {
        if let Some((status, detail)) = extract_embedded_upstream_error(payload) {
            return Err(LlmClientError::Upstream { status, detail });
        }

        let Some(event_type) = payload.get("type").and_then(Value::as_str) else {
            return Ok(());
        };

        match event_type {
            "response.output_text.delta" => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                    dispatcher.emit(LlmStreamEvent::TextDelta(delta.to_string()));
                }
            }
            "response.output_text.done" => {
                if let Some(text) = payload.get("text").and_then(Value::as_str) {
                    dispatcher.emit(LlmStreamEvent::TextDone(text.to_string()));
                }
            }
            "response.output_item.added" | "response.output_item.done" => {
                if let Some(item) = payload.get("item") {
                    remember_responses_function_call_item(item, &mut self.tool_states);
                    try_emit_responses_tool_call(
                        item.get("id").and_then(Value::as_str),
                        &mut self.tool_states,
                        &mut self.emitted_tool_calls,
                        dispatcher,
                    );
                }
            }
            "response.function_call_arguments.delta" => {
                let item_id = payload
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let delta = payload
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let state = self.tool_states.entry(item_id).or_default();
                state.arguments.push_str(delta.as_str());
                dispatcher.emit(LlmStreamEvent::ToolCallDelta {
                    call_id: state.call_id.clone(),
                    name: state.name.clone(),
                    arguments_delta: delta,
                });
            }
            "response.function_call_arguments.done" => {
                let item_id = payload
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let state = self.tool_states.entry(item_id.clone()).or_default();
                if let Some(name) = payload.get("name").and_then(Value::as_str) {
                    state.name = Some(name.to_string());
                }
                if let Some(arguments) = payload.get("arguments").and_then(Value::as_str) {
                    state.arguments = arguments.to_string();
                }
                try_emit_responses_tool_call(
                    Some(item_id.as_str()),
                    &mut self.tool_states,
                    &mut self.emitted_tool_calls,
                    dispatcher,
                );
            }
            "response.completed" => {
                self.completed_response = payload.get("response").cloned();
            }
            "response.failed" => {
                let response_error = payload.pointer("/response/error");
                let detail = response_error
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("response.failed")
                    .to_string();
                let status = response_error
                    .map(|error| infer_embedded_upstream_status(error.get("code"), detail.as_str()))
                    .unwrap_or_else(|| infer_embedded_upstream_status(None, detail.as_str()));
                return Err(LlmClientError::Upstream { status, detail });
            }
            "error" => {
                let detail = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("response stream error")
                    .to_string();
                let status = infer_embedded_upstream_status(payload.get("code"), detail.as_str());
                return Err(LlmClientError::Upstream { status, detail });
            }
            _ => {}
        }

        Ok(())
    }

    pub(super) fn finish(
        self,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let payload = self.completed_response.ok_or_else(|| {
            LlmClientError::InvalidResponse(
                "responses stream completed without response.completed event".to_string(),
            )
        })?;
        let mut emitted_tool_calls = self.emitted_tool_calls;
        Ok(finalize_responses_stream_turn(
            &payload,
            &mut emitted_tool_calls,
            dispatcher,
        ))
    }
}

#[derive(Default)]
pub(super) struct ChatStreamState {
    text: String,
    tool_calls: Vec<PendingToolCall>,
}

impl ChatStreamState {
    pub(super) fn apply_event(
        &mut self,
        payload: &Value,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<(), LlmClientError> {
        if let Some((status, detail)) = extract_embedded_upstream_error(payload) {
            return Err(LlmClientError::Upstream { status, detail });
        }

        apply_chat_stream_chunk(payload, &mut self.text, &mut self.tool_calls, dispatcher)
    }

    pub(super) fn finish(self, dispatcher: &mut EventDispatcher<'_>) -> ProviderTurn {
        finalize_chat_stream_turn(self.text, self.tool_calls, dispatcher)
    }
}

pub(super) fn remember_responses_function_call_item(
    item: &Value,
    tool_states: &mut std::collections::HashMap<String, ResponsesToolCallState>,
) {
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return;
    }

    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let state = tool_states.entry(item_id).or_default();
    state.call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| state.call_id.clone());
    state.name = item
        .get("name")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| state.name.clone());
    if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
        state.arguments = arguments.to_string();
    }
}

pub(super) fn try_emit_responses_tool_call(
    item_id: Option<&str>,
    tool_states: &mut std::collections::HashMap<String, ResponsesToolCallState>,
    emitted_tool_calls: &mut std::collections::HashSet<String>,
    dispatcher: &mut EventDispatcher<'_>,
) {
    let Some(item_id) = item_id else {
        return;
    };
    let Some(state) = tool_states.get(item_id) else {
        return;
    };
    let (Some(call_id), Some(name)) = (state.call_id.as_deref(), state.name.as_deref()) else {
        return;
    };
    if !emitted_tool_calls.insert(call_id.to_string()) {
        return;
    }

    dispatcher.emit(LlmStreamEvent::ToolCall {
        call_id: call_id.to_string(),
        name: name.to_string(),
        arguments: state.arguments.clone(),
    });
}

pub(super) fn apply_chat_stream_chunk(
    payload: &Value,
    text: &mut String,
    tool_calls: &mut Vec<PendingToolCall>,
    dispatcher: &mut EventDispatcher<'_>,
) -> Result<(), LlmClientError> {
    let Some(choices) = payload.get("choices").and_then(Value::as_array) else {
        return Ok(());
    };
    for choice in choices {
        let Some(delta) = choice.get("delta") else {
            continue;
        };

        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            text.push_str(content);
            dispatcher.emit(LlmStreamEvent::TextDelta(content.to_string()));
        }

        if let Some(tool_call_deltas) = delta.get("tool_calls").and_then(Value::as_array) {
            for tool_call_delta in tool_call_deltas {
                let index = tool_call_delta
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                while tool_calls.len() <= index {
                    tool_calls.push(PendingToolCall {
                        call_id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                }

                let Some(tool_call) = tool_calls.get_mut(index) else {
                    continue;
                };
                if let Some(call_id) = tool_call_delta.get("id").and_then(Value::as_str) {
                    tool_call.call_id = call_id.to_string();
                }
                if let Some(name) = tool_call_delta
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                {
                    tool_call.name = name.to_string();
                }
                if let Some(arguments_delta) = tool_call_delta
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                {
                    tool_call.arguments.push_str(arguments_delta);
                    dispatcher.emit(LlmStreamEvent::ToolCallDelta {
                        call_id: (!tool_call.call_id.is_empty()).then(|| tool_call.call_id.clone()),
                        name: (!tool_call.name.is_empty()).then(|| tool_call.name.clone()),
                        arguments_delta: arguments_delta.to_string(),
                    });
                }
            }
        }
    }

    Ok(())
}

pub(super) fn build_responses_turn_from_payload(
    payload: &Value,
    dispatcher: &mut EventDispatcher<'_>,
) -> ProviderTurn {
    let tool_calls = extract_responses_tool_calls(payload);
    for tool_call in &tool_calls {
        dispatcher.emit(LlmStreamEvent::ToolCall {
            call_id: tool_call.call_id.clone(),
            name: tool_call.name.clone(),
            arguments: tool_call.arguments.clone(),
        });
    }

    let text = extract_output_text(payload).unwrap_or_default();
    if !text.is_empty() {
        dispatcher.emit(LlmStreamEvent::TextDone(text.clone()));
    }

    ProviderTurn::responses(
        payload
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        text,
        tool_calls,
    )
}

pub(super) fn build_chat_turn_from_payload(
    payload: &Value,
    dispatcher: &mut EventDispatcher<'_>,
) -> ProviderTurn {
    let tool_calls = extract_chat_tool_calls(payload);
    for tool_call in &tool_calls {
        dispatcher.emit(LlmStreamEvent::ToolCall {
            call_id: tool_call.call_id.clone(),
            name: tool_call.name.clone(),
            arguments: tool_call.arguments.clone(),
        });
    }

    let text = extract_output_text(payload).unwrap_or_default();
    if !text.is_empty() {
        dispatcher.emit(LlmStreamEvent::TextDone(text.clone()));
    }

    ProviderTurn::chat(
        text,
        tool_calls,
        build_chat_assistant_message_from_payload(payload),
    )
}

pub(super) fn finalize_responses_stream_turn(
    payload: &Value,
    emitted_tool_calls: &mut std::collections::HashSet<String>,
    dispatcher: &mut EventDispatcher<'_>,
) -> ProviderTurn {
    let tool_calls = extract_responses_tool_calls(payload);
    for tool_call in &tool_calls {
        if emitted_tool_calls.insert(tool_call.call_id.clone()) {
            dispatcher.emit(LlmStreamEvent::ToolCall {
                call_id: tool_call.call_id.clone(),
                name: tool_call.name.clone(),
                arguments: tool_call.arguments.clone(),
            });
        }
    }

    ProviderTurn::responses(
        payload
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        extract_output_text(payload).unwrap_or_default(),
        tool_calls,
    )
}

pub(super) fn finalize_chat_stream_turn(
    text: String,
    tool_calls: Vec<PendingToolCall>,
    dispatcher: &mut EventDispatcher<'_>,
) -> ProviderTurn {
    for tool_call in &tool_calls {
        dispatcher.emit(LlmStreamEvent::ToolCall {
            call_id: tool_call.call_id.clone(),
            name: tool_call.name.clone(),
            arguments: tool_call.arguments.clone(),
        });
    }
    if !text.is_empty() {
        dispatcher.emit(LlmStreamEvent::TextDone(text.clone()));
    }

    let assistant_message = build_chat_assistant_message(text.clone(), tool_calls.clone());
    ProviderTurn::chat(text, tool_calls, assistant_message)
}

pub(super) fn build_chat_assistant_message_from_payload(payload: &Value) -> ChatMessage {
    if let Some(message) = payload.pointer("/choices/0/message") {
        return ChatMessage::Raw(message.clone());
    }

    let text = extract_output_text(payload).unwrap_or_default();
    let tool_calls = extract_chat_tool_calls(payload);
    build_chat_assistant_message(text, tool_calls)
}

fn build_chat_assistant_message(text: String, tool_calls: Vec<PendingToolCall>) -> ChatMessage {
    ChatMessage::assistant(text, tool_calls)
}

pub(super) fn extract_responses_tool_calls(payload: &Value) -> Vec<PendingToolCall> {
    payload
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .filter_map(|item| {
            Some(PendingToolCall {
                call_id: item.get("call_id")?.as_str()?.to_string(),
                name: item.get("name")?.as_str()?.to_string(),
                arguments: item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

pub(super) fn extract_chat_tool_calls(payload: &Value) -> Vec<PendingToolCall> {
    payload
        .pointer("/choices/0/message/tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool_call| {
            Some(PendingToolCall {
                call_id: tool_call.get("id")?.as_str()?.to_string(),
                name: tool_call.pointer("/function/name")?.as_str()?.to_string(),
                arguments: tool_call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

pub(super) fn extract_embedded_upstream_error(payload: &Value) -> Option<(u16, String)> {
    let error = payload.get("error")?;
    if let Some(message) = error.as_str() {
        let detail = message.trim();
        if detail.is_empty() {
            return None;
        }
        return Some((
            infer_embedded_upstream_status(None, detail),
            truncate_text(detail, OUTPUT_TRUNCATE_LIMIT),
        ));
    }

    let message = error.get("message").and_then(Value::as_str)?.trim();
    if message.is_empty() {
        return None;
    }

    let status = infer_embedded_upstream_status(error.get("code"), message);
    Some((status, truncate_text(message, OUTPUT_TRUNCATE_LIMIT)))
}

pub(super) fn infer_embedded_upstream_status(code: Option<&Value>, message: &str) -> u16 {
    parse_upstream_status_code(code)
        .or_else(|| should_fallback_to_chat_completions(400, message).then_some(400))
        .unwrap_or(500)
}

fn parse_upstream_status_code(code: Option<&Value>) -> Option<u16> {
    let code = code?;
    if let Some(status) = code
        .as_i64()
        .filter(|status| (100..=599).contains(status))
        .map(|status| status as u16)
    {
        return Some(status);
    }

    code.as_str()
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .filter(|status| (100..=599).contains(status))
}

pub(super) fn should_fallback_to_chat_completions(status: u16, detail: &str) -> bool {
    if matches!(status, 404 | 405) {
        return true;
    }
    if status != 400 {
        return false;
    }
    let normalized = detail.to_ascii_lowercase();
    normalized.contains("instruction") || normalized.contains("responses")
}

pub(crate) fn extract_output_text(payload: &Value) -> Option<String> {
    if let Some(output_text) = payload.get("output_text").and_then(Value::as_str) {
        return Some(output_text.trim().to_string()).filter(|value| !value.is_empty());
    }

    if let Some(output_text_items) = payload.get("output_text").and_then(Value::as_array) {
        let joined = output_text_items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !joined.is_empty() {
            return Some(joined);
        }
    }

    if let Some(output_items) = payload.get("output").and_then(Value::as_array) {
        let mut fragments = Vec::new();
        for item in output_items {
            let Some(contents) = item.get("content").and_then(Value::as_array) else {
                continue;
            };
            for content in contents {
                if let Some(text) = content.get("text").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        fragments.push(trimmed.to_string());
                    }
                }
            }
        }
        if !fragments.is_empty() {
            return Some(fragments.join("\n"));
        }
    }

    if let Some(chat_content) = payload.pointer("/choices/0/message/content") {
        if let Some(text) = chat_content.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        if let Some(items) = chat_content.as_array() {
            let joined = items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .or_else(|| item.get("text").and_then(Value::as_str))
                })
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if !joined.is_empty() {
                return Some(joined);
            }
        }
    }

    payload
        .pointer("/choices/0/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

pub(super) fn truncate_text(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let truncated = trimmed.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}
