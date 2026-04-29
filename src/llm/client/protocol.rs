use serde_json::{Value, json};

use super::{LlmClientError, LlmExecutedToolCall};

#[derive(Debug, Clone)]
enum ProviderContinuation {
    Responses { previous_response_id: String },
    Chat { assistant_message: ChatMessage },
}

#[derive(Debug, Clone)]
pub(super) struct ProviderTurn {
    pub(super) text: String,
    pub(super) tool_calls: Vec<PendingToolCall>,
    continuation: Option<ProviderContinuation>,
}

impl ProviderTurn {
    pub(super) fn responses(
        response_id: Option<String>,
        text: String,
        tool_calls: Vec<PendingToolCall>,
    ) -> Self {
        Self {
            text,
            tool_calls,
            continuation: response_id.map(|previous_response_id| ProviderContinuation::Responses {
                previous_response_id,
            }),
        }
    }

    pub(super) fn chat(
        text: String,
        tool_calls: Vec<PendingToolCall>,
        assistant_message: ChatMessage,
    ) -> Self {
        Self {
            text,
            tool_calls,
            continuation: Some(ProviderContinuation::Chat { assistant_message }),
        }
    }

    pub(super) fn into_responses_turn_input(
        &self,
        executed_tools: &[LlmExecutedToolCall],
    ) -> Result<ResponsesTurnInput, LlmClientError> {
        let Some(ProviderContinuation::Responses {
            previous_response_id,
        }) = self.continuation.as_ref()
        else {
            return Err(LlmClientError::InvalidResponse(
                "tool loop missing previous response id".to_string(),
            ));
        };

        Ok(ResponsesTurnInput::ToolOutputs {
            previous_response_id: previous_response_id.clone(),
            outputs: build_responses_tool_outputs(executed_tools),
        })
    }

    pub(super) fn extend_chat_history(
        &self,
        history: &mut ChatHistory,
        executed_tools: &[LlmExecutedToolCall],
    ) -> Result<(), LlmClientError> {
        let Some(ProviderContinuation::Chat { assistant_message }) = self.continuation.as_ref()
        else {
            return Err(LlmClientError::InvalidResponse(
                "chat tool loop missing assistant tool-call message".to_string(),
            ));
        };

        history.push(assistant_message.clone());
        history.extend(executed_tools.iter().map(build_chat_tool_message));
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ChatHistory {
    messages: Vec<ChatMessage>,
}

impl ChatHistory {
    // 外部调用
    #[allow(dead_code)]
    pub(super) fn from_messages(messages: Vec<Value>) -> Self {
        Self {
            messages: messages.into_iter().map(ChatMessage::Raw).collect(),
        }
    }

    pub(super) fn from_prompt(system_prompt: Option<&str>, prompt: Option<&str>) -> Self {
        let mut messages = Vec::new();
        if let Some(system_prompt) = system_prompt
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            messages.push(ChatMessage::Raw(json!({
                "role": "system",
                "content": system_prompt,
            })));
        }
        if let Some(prompt) = prompt.map(str::trim).filter(|value| !value.is_empty()) {
            messages.push(ChatMessage::Raw(json!({
                "role": "user",
                "content": prompt,
            })));
        }
        Self { messages }
    }

    pub(super) fn to_value_array(&self) -> Vec<Value> {
        self.messages.iter().map(ChatMessage::to_value).collect()
    }

    fn push(&mut self, message: ChatMessage) {
        self.messages.push(message);
    }

    fn extend<I>(&mut self, messages: I)
    where
        I: IntoIterator<Item = ChatMessage>,
    {
        self.messages.extend(messages);
    }
}

#[derive(Debug, Clone)]
pub(super) enum ChatMessage {
    Raw(Value),
    Assistant(ChatAssistantMessage),
    Tool(ChatToolMessage),
}

impl ChatMessage {
    pub(super) fn assistant(text: String, tool_calls: Vec<PendingToolCall>) -> Self {
        Self::Assistant(ChatAssistantMessage { text, tool_calls })
    }

    pub(super) fn to_value(&self) -> Value {
        match self {
            Self::Raw(value) => value.clone(),
            Self::Assistant(message) => message.to_value(),
            Self::Tool(message) => message.to_value(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ChatAssistantMessage {
    text: String,
    tool_calls: Vec<PendingToolCall>,
}

impl ChatAssistantMessage {
    fn to_value(&self) -> Value {
        let mut message = serde_json::Map::new();
        message.insert("role".to_string(), Value::String("assistant".to_string()));
        if self.tool_calls.is_empty() {
            message.insert("content".to_string(), Value::String(self.text.clone()));
            return Value::Object(message);
        }

        if self.text.is_empty() {
            message.insert("content".to_string(), Value::Null);
        } else {
            message.insert("content".to_string(), Value::String(self.text.clone()));
        }
        message.insert(
            "tool_calls".to_string(),
            Value::Array(
                self.tool_calls
                    .iter()
                    .map(PendingToolCall::to_chat_tool_call_value)
                    .collect(),
            ),
        );
        Value::Object(message)
    }
}

#[derive(Debug, Clone)]
pub(super) struct ChatToolMessage {
    tool_call_id: String,
    content: String,
}

impl ChatToolMessage {
    fn to_value(&self) -> Value {
        json!({
            "role": "tool",
            "tool_call_id": self.tool_call_id,
            "content": self.content,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PendingToolCall {
    pub(super) call_id: String,
    pub(super) name: String,
    pub(super) arguments: String,
}

impl PendingToolCall {
    fn to_chat_tool_call_value(&self) -> Value {
        json!({
            "id": self.call_id,
            "type": "function",
            "function": {
                "name": self.name,
                "arguments": self.arguments,
            }
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ResponsesToolCallState {
    pub(super) call_id: Option<String>,
    pub(super) name: Option<String>,
    pub(super) arguments: String,
}

#[derive(Debug, Clone)]
pub(super) enum ResponsesTurnInput {
    Initial {
        input: Value,
    },
    ToolOutputs {
        previous_response_id: String,
        outputs: Vec<ResponsesToolOutput>,
    },
}

#[derive(Debug, Clone)]
pub(super) struct ResponsesToolOutput {
    call_id: String,
    output: String,
}

impl ResponsesToolOutput {
    pub(super) fn to_value(&self) -> Value {
        json!({
            "type": "function_call_output",
            "call_id": self.call_id,
            "output": self.output,
        })
    }
}

fn build_responses_tool_outputs(
    executed_tools: &[LlmExecutedToolCall],
) -> Vec<ResponsesToolOutput> {
    executed_tools
        .iter()
        .map(|tool_call| ResponsesToolOutput {
            call_id: tool_call.call_id.clone(),
            output: tool_call.output.clone(),
        })
        .collect()
}

fn build_chat_tool_message(tool_call: &LlmExecutedToolCall) -> ChatMessage {
    ChatMessage::Tool(ChatToolMessage {
        tool_call_id: tool_call.call_id.clone(),
        content: tool_call.output.clone(),
    })
}
