use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::Value;

#[path = "client/event_dispatch.rs"]
mod event_dispatch;
#[path = "client/protocol.rs"]
mod protocol;
#[path = "client/request_encoding.rs"]
mod request_encoding;
#[path = "client/response_parsing.rs"]
mod response_parsing;
#[path = "client/session_runtime.rs"]
mod session_runtime;
#[path = "client/transport.rs"]
mod transport;

#[cfg(test)]
use self::request_encoding::{build_responses_request, llm_endpoint};
#[cfg(test)]
use self::response_parsing::apply_chat_stream_chunk;
use self::response_parsing::{append_turn_text, should_fallback_to_chat_completions};

const OUTPUT_TRUNCATE_LIMIT: usize = 320;

pub trait OpenAiRuntimeConfig {
    fn base_url(&self) -> &str;
    fn model(&self) -> &str;
    fn timeout_ms(&self) -> u64;
    fn system_prompt(&self) -> Option<&str>;

    fn stream(&self) -> bool {
        false
    }

    fn temperature(&self) -> Option<f32> {
        None
    }

    fn top_p(&self) -> Option<f32> {
        None
    }

    fn top_k(&self) -> Option<u32> {
        None
    }

    fn frequency_penalty(&self) -> Option<f32> {
        None
    }

    fn presence_penalty(&self) -> Option<f32> {
        None
    }

    fn parallel_tool_calls(&self) -> bool {
        true
    }

    fn reasoning_effort(&self) -> Option<&str> {
        None
    }

    fn default_headers(&self) -> Option<&HashMap<String, String>> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiResponsesClient {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    system_prompt: Option<String>,
    stream: bool,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    frequency_penalty: Option<f32>,
    presence_penalty: Option<f32>,
    parallel_tool_calls: bool,
    reasoning_effort: Option<String>,
    default_headers: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmCompletion {
    pub text: String,
    pub tool_calls: Vec<LlmExecutedToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmExecutedToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmStreamEvent {
    TextDelta(String),
    TextDone(String),
    ToolCallDelta {
        call_id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    ToolOutput {
        call_id: String,
        name: String,
        output: String,
    },
}

pub trait LlmEventSink: Send {
    fn on_event(&mut self, event: LlmStreamEvent);
}

impl<F> LlmEventSink for F
where
    F: FnMut(LlmStreamEvent) + Send,
{
    fn on_event(&mut self, event: LlmStreamEvent) {
        self(event);
    }
}

pub type LlmToolFuture =
    Pin<Box<dyn Future<Output = Result<LlmToolOutput, LlmClientError>> + Send>>;
pub type LlmToolHandler = Arc<dyn Fn(Value) -> LlmToolFuture + Send + Sync>;

#[derive(Clone)]
pub struct LlmFunctionTool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
    pub strict: bool,
    handler: LlmToolHandler,
}

impl std::fmt::Debug for LlmFunctionTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmFunctionTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("parameters", &self.parameters)
            .field("strict", &self.strict)
            .finish()
    }
}

// 外部调用
#[allow(dead_code)]
impl LlmFunctionTool {
    pub fn new<F, Fut>(name: impl Into<String>, parameters: Value, handler: F) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<LlmToolOutput, LlmClientError>> + Send + 'static,
    {
        Self {
            name: name.into(),
            description: None,
            parameters,
            strict: true,
            handler: Arc::new(move |arguments| Box::pin(handler(arguments))),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

// 外部调用
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum LlmToolOutput {
    Text(String),
    Json(Value),
}

impl LlmToolOutput {
    fn into_output_string(self) -> Result<String, LlmClientError> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Json(value) => serde_json::to_string(&value).map_err(|err| {
                LlmClientError::InvalidResponse(format!(
                    "failed to serialize tool output as json: {err}"
                ))
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LlmClientError {
    NotConfigured(String),
    Http(String),
    Upstream { status: u16, detail: String },
    InvalidResponse(String),
    Tool(String),
}

impl std::fmt::Display for LlmClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(reason) => write!(f, "LLM not configured: {reason}"),
            Self::Http(reason) => write!(f, "LLM HTTP request failed: {reason}"),
            Self::Upstream { status, detail } => {
                write!(f, "LLM upstream failed (status={status}): {detail}")
            }
            Self::InvalidResponse(reason) => write!(f, "LLM response invalid: {reason}"),
            Self::Tool(reason) => write!(f, "LLM tool execution failed: {reason}"),
        }
    }
}

impl std::error::Error for LlmClientError {}

impl OpenAiResponsesClient {
    pub fn from_runtime_with_api_key(
        config: &impl OpenAiRuntimeConfig,
        api_key: &str,
    ) -> Result<Self, LlmClientError> {
        let api_key = api_key.trim().to_string();
        if api_key.is_empty() {
            return Err(LlmClientError::NotConfigured(
                "missing api key for current request".to_string(),
            ));
        }

        let client = Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms().max(10)))
            .build()
            .map_err(|err| LlmClientError::NotConfigured(format!("reqwest init failed: {err}")))?;

        Ok(Self {
            client,
            base_url: config.base_url().trim_end_matches('/').to_string(),
            api_key,
            model: config.model().to_string(),
            system_prompt: config.system_prompt().map(ToString::to_string),
            stream: config.stream(),
            temperature: config.temperature(),
            top_p: config.top_p(),
            top_k: config.top_k(),
            frequency_penalty: config.frequency_penalty(),
            presence_penalty: config.presence_penalty(),
            parallel_tool_calls: config.parallel_tool_calls(),
            reasoning_effort: config
                .reasoning_effort()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            default_headers: config.default_headers().cloned().unwrap_or_default(),
        })
    }
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn extract_output_text(payload: &Value) -> Option<String> {
    response_parsing::extract_output_text(payload)
}

#[cfg(test)]
#[path = "client/tests.rs"]
mod tests;
