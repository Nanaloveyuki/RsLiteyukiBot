use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{Map, Value, json};

use liteyukibot_core::{LogLevel, SseParser, emit_console_log};

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

    pub async fn generate(&self, prompt: &str) -> Result<String, LlmClientError> {
        let sink: Option<&mut dyn LlmEventSink> = None;
        self.complete(prompt, &[], sink)
            .await
            .map(|completion| completion.text)
    }

    #[allow(dead_code)]
    pub async fn generate_with_events(
        &self,
        prompt: &str,
        sink: &mut dyn LlmEventSink,
    ) -> Result<LlmCompletion, LlmClientError> {
        self.complete(prompt, &[], Some(sink)).await
    }

    pub async fn complete(
        &self,
        prompt: &str,
        tools: &[LlmFunctionTool],
        sink: Option<&mut dyn LlmEventSink>,
    ) -> Result<LlmCompletion, LlmClientError> {
        self.complete_with_input(
            prompt,
            Value::String(prompt.trim().to_string()),
            tools,
            sink,
        )
        .await
    }

    pub async fn complete_with_input(
        &self,
        prompt_fallback: &str,
        input: Value,
        tools: &[LlmFunctionTool],
        sink: Option<&mut dyn LlmEventSink>,
    ) -> Result<LlmCompletion, LlmClientError> {
        let prompt = prompt_fallback.trim().to_string();
        let mut dispatcher = EventDispatcher::new(sink);
        let stream = self.stream || dispatcher.has_sink();
        let mut executed_tools = Vec::new();
        let mut accumulated_text = String::new();

        let initial = self
            .start_session(prompt, input, tools, stream, &mut dispatcher)
            .await?;
        let mut session = initial.session;
        let mut turn = initial.turn;

        loop {
            append_turn_text(&mut accumulated_text, turn.text.as_str());

            if turn.tool_calls.is_empty() {
                return Ok(LlmCompletion {
                    text: accumulated_text,
                    tool_calls: executed_tools,
                });
            }

            let (outputs, executed) =
                execute_tool_calls(&turn.tool_calls, tools, &mut dispatcher).await?;
            executed_tools.extend(executed);
            turn = session
                .continue_with_tool_outputs(self, &turn, outputs, tools, stream, &mut dispatcher)
                .await?;
        }
    }

    pub async fn generate_with_chat_completions(
        &self,
        prompt: &str,
    ) -> Result<String, LlmClientError> {
        let sink: Option<&mut dyn LlmEventSink> = None;
        self.complete_with_chat_completions(prompt, &[], sink)
            .await
            .map(|completion| completion.text)
    }

    pub async fn complete_with_chat_completions(
        &self,
        prompt: &str,
        tools: &[LlmFunctionTool],
        sink: Option<&mut dyn LlmEventSink>,
    ) -> Result<LlmCompletion, LlmClientError> {
        let history = initial_chat_history(self.system_prompt.as_deref(), Some(prompt));
        self.complete_with_chat_messages(history, tools, sink).await
    }

    pub async fn complete_with_chat_messages(
        &self,
        history: Vec<Value>,
        tools: &[LlmFunctionTool],
        sink: Option<&mut dyn LlmEventSink>,
    ) -> Result<LlmCompletion, LlmClientError> {
        let mut dispatcher = EventDispatcher::new(sink);
        let stream = self.stream || dispatcher.has_sink();
        let mut executed_tools = Vec::new();
        let mut accumulated_text = String::new();
        let mut session = ProviderSession::Chat {
            history: history.clone(),
        };
        let mut turn = self
            .send_chat_turn(history.as_slice(), tools, stream, &mut dispatcher)
            .await?;

        loop {
            append_turn_text(&mut accumulated_text, turn.text.as_str());

            if turn.tool_calls.is_empty() {
                return Ok(LlmCompletion {
                    text: accumulated_text,
                    tool_calls: executed_tools,
                });
            }

            let (outputs, executed) =
                execute_tool_calls(&turn.tool_calls, tools, &mut dispatcher).await?;
            executed_tools.extend(executed);
            turn = session
                .continue_with_tool_outputs(self, &turn, outputs, tools, stream, &mut dispatcher)
                .await?;
        }
    }

    async fn start_session(
        &self,
        prompt_fallback: String,
        input: Value,
        tools: &[LlmFunctionTool],
        stream: bool,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderStart, LlmClientError> {
        match self
            .send_responses_turn(
                ResponsesTurnInput::Initial { input },
                tools,
                stream,
                dispatcher,
            )
            .await
        {
            Ok(turn) => Ok(ProviderStart {
                session: ProviderSession::Responses,
                turn,
            }),
            Err(LlmClientError::Upstream { status, detail })
                if should_fallback_to_chat_completions(status, detail.as_str()) =>
            {
                let history = initial_chat_history(
                    self.system_prompt.as_deref(),
                    Some(prompt_fallback.as_str()),
                );
                let turn = self
                    .send_chat_turn(history.as_slice(), tools, stream, dispatcher)
                    .await?;
                Ok(ProviderStart {
                    session: ProviderSession::Chat { history },
                    turn,
                })
            }
            Err(err) => Err(err),
        }
    }

    async fn send_responses_turn(
        &self,
        input: ResponsesTurnInput,
        tools: &[LlmFunctionTool],
        stream: bool,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let request = self.build_responses_request(&input, tools, stream);
        let endpoint = llm_endpoint(self.base_url.as_str(), "responses");
        if stream {
            self.send_responses_stream(endpoint.as_str(), request, dispatcher)
                .await
        } else {
            self.send_responses_json(endpoint.as_str(), request, dispatcher)
                .await
        }
    }

    async fn send_chat_turn(
        &self,
        history: &[Value],
        tools: &[LlmFunctionTool],
        stream: bool,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let request = self.build_chat_request(history, tools, stream);
        let endpoint = llm_endpoint(self.base_url.as_str(), "chat/completions");
        if stream {
            self.send_chat_stream(endpoint.as_str(), request, dispatcher)
                .await
        } else {
            self.send_chat_json(endpoint.as_str(), request, dispatcher)
                .await
        }
    }

    async fn send_responses_json(
        &self,
        endpoint: &str,
        request: Value,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let payload = self.send_json(endpoint, &request).await?;
        if let Some((status, detail)) = extract_embedded_upstream_error(&payload) {
            return Err(LlmClientError::Upstream { status, detail });
        }

        let tool_calls = extract_responses_tool_calls(&payload);
        for tool_call in &tool_calls {
            dispatcher.emit(LlmStreamEvent::ToolCall {
                call_id: tool_call.call_id.clone(),
                name: tool_call.name.clone(),
                arguments: tool_call.arguments.clone(),
            });
        }

        let text = extract_output_text(&payload).unwrap_or_default();
        if !text.is_empty() {
            dispatcher.emit(LlmStreamEvent::TextDone(text.clone()));
        }

        Ok(ProviderTurn {
            response_id: payload
                .get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            text,
            tool_calls,
            assistant_message: None,
        })
    }

    async fn send_chat_json(
        &self,
        endpoint: &str,
        request: Value,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let payload = self.send_json(endpoint, &request).await?;
        if let Some((status, detail)) = extract_embedded_upstream_error(&payload) {
            return Err(LlmClientError::Upstream { status, detail });
        }

        let tool_calls = extract_chat_tool_calls(&payload);
        for tool_call in &tool_calls {
            dispatcher.emit(LlmStreamEvent::ToolCall {
                call_id: tool_call.call_id.clone(),
                name: tool_call.name.clone(),
                arguments: tool_call.arguments.clone(),
            });
        }

        let text = extract_output_text(&payload).unwrap_or_default();
        if !text.is_empty() {
            dispatcher.emit(LlmStreamEvent::TextDone(text.clone()));
        }

        Ok(ProviderTurn {
            response_id: None,
            text,
            tool_calls,
            assistant_message: Some(build_chat_assistant_message_from_payload(&payload)),
        })
    }

    async fn send_responses_stream(
        &self,
        endpoint: &str,
        request: Value,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        log_outbound_llm_request("responses", endpoint, &request);
        let response = self
            .apply_default_headers(self.client.post(endpoint).bearer_auth(&self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|err| LlmClientError::Http(err.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .map_err(|err| LlmClientError::Http(err.to_string()))?;
            return Err(LlmClientError::Upstream {
                status: status.as_u16(),
                detail: truncate_text(body.trim(), OUTPUT_TRUNCATE_LIMIT),
            });
        }

        let mut parser = SseParser::default();
        let mut stream = response.bytes_stream();
        let mut completed_response = None;
        let mut tool_states = HashMap::<String, ResponsesToolCallState>::new();
        let mut emitted_tool_calls = HashSet::<String>::new();

        while let Some(next) = stream.next().await {
            let chunk = next.map_err(|err| LlmClientError::Http(err.to_string()))?;
            let chunk_text = String::from_utf8_lossy(&chunk);
            for event in parser.push_chunk(&chunk_text) {
                let data = event.data.trim();
                if data.is_empty() {
                    continue;
                }

                let payload: Value = serde_json::from_str(data)
                    .map_err(|err| LlmClientError::InvalidResponse(err.to_string()))?;
                if let Some((status, detail)) = extract_embedded_upstream_error(&payload) {
                    return Err(LlmClientError::Upstream { status, detail });
                }

                let Some(event_type) = payload.get("type").and_then(Value::as_str) else {
                    continue;
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
                            remember_responses_function_call_item(item, &mut tool_states);
                            try_emit_responses_tool_call(
                                item.get("id").and_then(Value::as_str),
                                &mut tool_states,
                                &mut emitted_tool_calls,
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
                        let state = tool_states.entry(item_id.clone()).or_default();
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
                        let state = tool_states.entry(item_id.clone()).or_default();
                        if let Some(name) = payload.get("name").and_then(Value::as_str) {
                            state.name = Some(name.to_string());
                        }
                        if let Some(arguments) = payload.get("arguments").and_then(Value::as_str) {
                            state.arguments = arguments.to_string();
                        }
                        try_emit_responses_tool_call(
                            Some(item_id.as_str()),
                            &mut tool_states,
                            &mut emitted_tool_calls,
                            dispatcher,
                        );
                    }
                    "response.completed" => {
                        completed_response = payload.get("response").cloned();
                    }
                    "response.failed" => {
                        let response_error = payload.pointer("/response/error");
                        let detail = response_error
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("response.failed")
                            .to_string();
                        let status = response_error
                            .map(|error| {
                                infer_embedded_upstream_status(error.get("code"), detail.as_str())
                            })
                            .unwrap_or_else(|| {
                                infer_embedded_upstream_status(None, detail.as_str())
                            });
                        return Err(LlmClientError::Upstream { status, detail });
                    }
                    "error" => {
                        let detail = payload
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("response stream error")
                            .to_string();
                        let status =
                            infer_embedded_upstream_status(payload.get("code"), detail.as_str());
                        return Err(LlmClientError::Upstream { status, detail });
                    }
                    _ => {}
                }
            }
        }

        let payload = completed_response.ok_or_else(|| {
            LlmClientError::InvalidResponse(
                "responses stream completed without response.completed event".to_string(),
            )
        })?;

        let tool_calls = extract_responses_tool_calls(&payload);
        for tool_call in &tool_calls {
            if emitted_tool_calls.insert(tool_call.call_id.clone()) {
                dispatcher.emit(LlmStreamEvent::ToolCall {
                    call_id: tool_call.call_id.clone(),
                    name: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                });
            }
        }

        Ok(ProviderTurn {
            response_id: payload
                .get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            text: extract_output_text(&payload).unwrap_or_default(),
            tool_calls,
            assistant_message: None,
        })
    }

    async fn send_chat_stream(
        &self,
        endpoint: &str,
        request: Value,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        log_outbound_llm_request("chat.completions", endpoint, &request);
        let response = self
            .apply_default_headers(self.client.post(endpoint).bearer_auth(&self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|err| LlmClientError::Http(err.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .map_err(|err| LlmClientError::Http(err.to_string()))?;
            return Err(LlmClientError::Upstream {
                status: status.as_u16(),
                detail: truncate_text(body.trim(), OUTPUT_TRUNCATE_LIMIT),
            });
        }

        let mut parser = SseParser::default();
        let mut stream = response.bytes_stream();
        let mut text = String::new();
        let mut tool_calls = Vec::<PendingToolCall>::new();

        while let Some(next) = stream.next().await {
            let chunk = next.map_err(|err| LlmClientError::Http(err.to_string()))?;
            let chunk_text = String::from_utf8_lossy(&chunk);
            for event in parser.push_chunk(&chunk_text) {
                let data = event.data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }

                let payload: Value = serde_json::from_str(data)
                    .map_err(|err| LlmClientError::InvalidResponse(err.to_string()))?;
                if let Some((status, detail)) = extract_embedded_upstream_error(&payload) {
                    return Err(LlmClientError::Upstream { status, detail });
                }
                apply_chat_stream_chunk(&payload, &mut text, &mut tool_calls, dispatcher)?;
            }
        }

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

        Ok(ProviderTurn {
            response_id: None,
            text: text.clone(),
            tool_calls: tool_calls.clone(),
            assistant_message: Some(build_chat_assistant_message(text, &tool_calls)),
        })
    }

    async fn send_json(&self, endpoint: &str, request: &Value) -> Result<Value, LlmClientError> {
        let request_kind = if endpoint
            .trim_end_matches('/')
            .ends_with("/chat/completions")
        {
            "chat.completions"
        } else if endpoint.trim_end_matches('/').ends_with("/responses") {
            "responses"
        } else {
            "json"
        };
        log_outbound_llm_request(request_kind, endpoint, request);
        let response = self
            .apply_default_headers(self.client.post(endpoint).bearer_auth(&self.api_key))
            .json(request)
            .send()
            .await
            .map_err(|err| LlmClientError::Http(err.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| LlmClientError::Http(err.to_string()))?;

        if !status.is_success() {
            return Err(LlmClientError::Upstream {
                status: status.as_u16(),
                detail: truncate_text(body.trim(), OUTPUT_TRUNCATE_LIMIT),
            });
        }

        let payload = serde_json::from_str(&body)
            .map_err(|err| LlmClientError::InvalidResponse(err.to_string()))?;
        if let Some((status, detail)) = extract_embedded_upstream_error(&payload) {
            return Err(LlmClientError::Upstream { status, detail });
        }
        Ok(payload)
    }

    fn apply_default_headers(
        &self,
        mut request: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        for (name, value) in &self.default_headers {
            request = request.header(name, value);
        }
        request
    }

    fn build_responses_request(
        &self,
        input: &ResponsesTurnInput,
        tools: &[LlmFunctionTool],
        stream: bool,
    ) -> Value {
        let mut body = Map::new();
        body.insert("model".to_string(), Value::String(self.model.clone()));

        if let Some(instructions) = self
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

        match input {
            ResponsesTurnInput::Initial { input } => {
                body.insert("input".to_string(), input.clone());
            }
            ResponsesTurnInput::ToolOutputs {
                previous_response_id,
                outputs,
            } => {
                body.insert(
                    "previous_response_id".to_string(),
                    Value::String(previous_response_id.clone()),
                );
                body.insert("input".to_string(), Value::Array(outputs.clone()));
            }
        }

        if stream {
            body.insert("stream".to_string(), Value::Bool(true));
            body.insert(
                "stream_options".to_string(),
                json!({ "include_obfuscation": false }),
            );
        }
        if let Some(temperature) = self.temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(top_p) = self.top_p {
            body.insert("top_p".to_string(), json!(top_p));
        }
        if let Some(top_k) = self
            .top_k
            .filter(|_| supports_compat_top_k(self.base_url.as_str()))
        {
            body.insert("top_k".to_string(), json!(top_k));
        }
        if let Some(frequency_penalty) = self.frequency_penalty {
            body.insert("frequency_penalty".to_string(), json!(frequency_penalty));
        }
        if let Some(presence_penalty) = self.presence_penalty {
            body.insert("presence_penalty".to_string(), json!(presence_penalty));
        }
        if let Some(reasoning_effort) = self.reasoning_effort.as_deref() {
            body.insert(
                "reasoning".to_string(),
                json!({ "effort": reasoning_effort }),
            );
        }
        if !tools.is_empty() {
            body.insert(
                "parallel_tool_calls".to_string(),
                Value::Bool(self.parallel_tool_calls),
            );
            body.insert(
                "tools".to_string(),
                Value::Array(tools.iter().map(encode_responses_tool).collect()),
            );
        }

        Value::Object(body)
    }

    fn build_chat_request(
        &self,
        messages: &[Value],
        tools: &[LlmFunctionTool],
        stream: bool,
    ) -> Value {
        let mut body = Map::new();
        body.insert("model".to_string(), Value::String(self.model.clone()));
        body.insert("messages".to_string(), Value::Array(messages.to_vec()));

        if stream {
            body.insert("stream".to_string(), Value::Bool(true));
        }
        if let Some(temperature) = self.temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(top_p) = self.top_p {
            body.insert("top_p".to_string(), json!(top_p));
        }
        if let Some(top_k) = self
            .top_k
            .filter(|_| supports_compat_top_k(self.base_url.as_str()))
        {
            body.insert("top_k".to_string(), json!(top_k));
        }
        if let Some(frequency_penalty) = self.frequency_penalty {
            body.insert("frequency_penalty".to_string(), json!(frequency_penalty));
        }
        if let Some(presence_penalty) = self.presence_penalty {
            body.insert("presence_penalty".to_string(), json!(presence_penalty));
        }
        if let Some(reasoning_effort) = self.reasoning_effort.as_deref() {
            body.insert(
                "reasoning".to_string(),
                json!({ "effort": reasoning_effort }),
            );
        }
        if !tools.is_empty() {
            body.insert(
                "parallel_tool_calls".to_string(),
                Value::Bool(self.parallel_tool_calls),
            );
            body.insert(
                "tools".to_string(),
                Value::Array(tools.iter().map(encode_chat_tool).collect()),
            );
        }

        Value::Object(body)
    }
}

#[derive(Debug, Clone)]
enum ProviderSession {
    Responses,
    Chat { history: Vec<Value> },
}

impl ProviderSession {
    async fn continue_with_tool_outputs(
        &mut self,
        client: &OpenAiResponsesClient,
        previous_turn: &ProviderTurn,
        outputs: Vec<Value>,
        tools: &[LlmFunctionTool],
        stream: bool,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        match self {
            Self::Responses => {
                let previous_response_id = previous_turn.response_id.clone().ok_or_else(|| {
                    LlmClientError::InvalidResponse(
                        "tool loop missing previous response id".to_string(),
                    )
                })?;
                client
                    .send_responses_turn(
                        ResponsesTurnInput::ToolOutputs {
                            previous_response_id,
                            outputs,
                        },
                        tools,
                        stream,
                        dispatcher,
                    )
                    .await
            }
            Self::Chat { history } => {
                let assistant_message =
                    previous_turn.assistant_message.clone().ok_or_else(|| {
                        LlmClientError::InvalidResponse(
                            "chat tool loop missing assistant tool-call message".to_string(),
                        )
                    })?;
                history.push(assistant_message);
                history.extend(outputs.into_iter().filter_map(build_chat_tool_message));
                client
                    .send_chat_turn(history.as_slice(), tools, stream, dispatcher)
                    .await
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ProviderStart {
    session: ProviderSession,
    turn: ProviderTurn,
}

#[derive(Debug, Clone)]
struct ProviderTurn {
    response_id: Option<String>,
    text: String,
    tool_calls: Vec<PendingToolCall>,
    assistant_message: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PendingToolCall {
    call_id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Default)]
struct ResponsesToolCallState {
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
}

#[derive(Debug, Clone)]
enum ResponsesTurnInput {
    Initial {
        input: Value,
    },
    ToolOutputs {
        previous_response_id: String,
        outputs: Vec<Value>,
    },
}

struct EventDispatcher<'a> {
    sink: Option<&'a mut dyn LlmEventSink>,
}

impl<'a> EventDispatcher<'a> {
    fn new(sink: Option<&'a mut dyn LlmEventSink>) -> Self {
        Self { sink }
    }

    fn emit(&mut self, event: LlmStreamEvent) {
        if let Some(sink) = self.sink.as_mut() {
            (*sink).on_event(event);
        }
    }

    fn has_sink(&self) -> bool {
        self.sink.is_some()
    }
}

fn append_turn_text(output: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(text);
}

fn encode_responses_tool(tool: &LlmFunctionTool) -> Value {
    let mut value = Map::new();
    value.insert("type".to_string(), Value::String("function".to_string()));
    value.insert("name".to_string(), Value::String(tool.name.clone()));
    value.insert(
        "parameters".to_string(),
        normalize_openai_tool_parameters(&tool.parameters, tool.strict),
    );
    value.insert("strict".to_string(), Value::Bool(tool.strict));
    if let Some(description) = tool.description.as_ref() {
        value.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    Value::Object(value)
}

fn encode_chat_tool(tool: &LlmFunctionTool) -> Value {
    let mut function = Map::new();
    function.insert("name".to_string(), Value::String(tool.name.clone()));
    function.insert(
        "parameters".to_string(),
        normalize_openai_tool_parameters(&tool.parameters, tool.strict),
    );
    function.insert("strict".to_string(), Value::Bool(tool.strict));
    if let Some(description) = tool.description.as_ref() {
        function.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    json!({
        "type": "function",
        "function": Value::Object(function),
    })
}

fn initial_chat_history(system_prompt: Option<&str>, prompt: Option<&str>) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(system_prompt) = system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(json!({
            "role": "system",
            "content": system_prompt,
        }));
    }
    if let Some(prompt) = prompt.map(str::trim).filter(|value| !value.is_empty()) {
        messages.push(json!({
            "role": "user",
            "content": prompt,
        }));
    }
    messages
}

async fn execute_tool_calls(
    tool_calls: &[PendingToolCall],
    tools: &[LlmFunctionTool],
    dispatcher: &mut EventDispatcher<'_>,
) -> Result<(Vec<Value>, Vec<LlmExecutedToolCall>), LlmClientError> {
    let mut outputs = Vec::new();
    let mut executed = Vec::new();

    for tool_call in tool_calls {
        let tool = tools
            .iter()
            .find(|tool| tool.name == tool_call.name)
            .ok_or_else(|| {
                LlmClientError::Tool(format!("tool '{}' is not registered", tool_call.name))
            })?;
        let arguments =
            serde_json::from_str::<Value>(tool_call.arguments.as_str()).map_err(|err| {
                LlmClientError::Tool(format!(
                    "tool '{}' received invalid json arguments: {err}",
                    tool_call.name
                ))
            })?;
        let output = (tool.handler)(arguments).await?.into_output_string()?;

        dispatcher.emit(LlmStreamEvent::ToolOutput {
            call_id: tool_call.call_id.clone(),
            name: tool_call.name.clone(),
            output: output.clone(),
        });
        executed.push(LlmExecutedToolCall {
            call_id: tool_call.call_id.clone(),
            name: tool_call.name.clone(),
            arguments: tool_call.arguments.clone(),
            output: output.clone(),
        });
        outputs.push(json!({
            "type": "function_call_output",
            "call_id": tool_call.call_id,
            "output": output,
        }));
    }

    Ok((outputs, executed))
}

fn remember_responses_function_call_item(
    item: &Value,
    tool_states: &mut HashMap<String, ResponsesToolCallState>,
) {
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return;
    }
    let Some(item_id) = item.get("id").and_then(Value::as_str) else {
        return;
    };

    let state = tool_states.entry(item_id.to_string()).or_default();
    if let Some(call_id) = item.get("call_id").and_then(Value::as_str) {
        state.call_id = Some(call_id.to_string());
    }
    if let Some(name) = item.get("name").and_then(Value::as_str) {
        state.name = Some(name.to_string());
    }
    if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
        state.arguments = arguments.to_string();
    }
}

fn try_emit_responses_tool_call(
    item_id: Option<&str>,
    tool_states: &mut HashMap<String, ResponsesToolCallState>,
    emitted_tool_calls: &mut HashSet<String>,
    dispatcher: &mut EventDispatcher<'_>,
) {
    let Some(item_id) = item_id else {
        return;
    };
    let Some(state) = tool_states.get(item_id) else {
        return;
    };
    let (Some(call_id), Some(name)) = (state.call_id.as_ref(), state.name.as_ref()) else {
        return;
    };
    if emitted_tool_calls.insert(call_id.clone()) {
        dispatcher.emit(LlmStreamEvent::ToolCall {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments: state.arguments.clone(),
        });
    }
}

fn apply_chat_stream_chunk(
    payload: &Value,
    text: &mut String,
    tool_calls: &mut Vec<PendingToolCall>,
    dispatcher: &mut EventDispatcher<'_>,
) -> Result<(), LlmClientError> {
    let Some(choices) = payload.get("choices").and_then(Value::as_array) else {
        return Ok(());
    };
    let Some(choice) = choices.first() else {
        return Ok(());
    };
    let Some(delta) = choice.get("delta") else {
        return Ok(());
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
                tool_calls.push(PendingToolCall::default());
            }

            let call = &mut tool_calls[index];
            if let Some(call_id) = tool_call_delta.get("id").and_then(Value::as_str) {
                call.call_id = call_id.to_string();
            }
            if let Some(function) = tool_call_delta.get("function") {
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    call.name = name.to_string();
                }
                if let Some(arguments_delta) = function.get("arguments").and_then(Value::as_str) {
                    call.arguments.push_str(arguments_delta);
                    dispatcher.emit(LlmStreamEvent::ToolCallDelta {
                        call_id: if call.call_id.is_empty() {
                            None
                        } else {
                            Some(call.call_id.clone())
                        },
                        name: if call.name.is_empty() {
                            None
                        } else {
                            Some(call.name.clone())
                        },
                        arguments_delta: arguments_delta.to_string(),
                    });
                }
            }
        }
    }

    Ok(())
}

fn build_chat_assistant_message_from_payload(payload: &Value) -> Value {
    if let Some(message) = payload.pointer("/choices/0/message") {
        return message.clone();
    }
    build_chat_assistant_message(
        extract_output_text(payload).unwrap_or_default(),
        &extract_chat_tool_calls(payload),
    )
}

fn build_chat_assistant_message(text: String, tool_calls: &[PendingToolCall]) -> Value {
    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    if tool_calls.is_empty() {
        message.insert("content".to_string(), Value::String(text));
        return Value::Object(message);
    }

    if text.is_empty() {
        message.insert("content".to_string(), Value::Null);
    } else {
        message.insert("content".to_string(), Value::String(text));
    }
    message.insert(
        "tool_calls".to_string(),
        Value::Array(
            tool_calls
                .iter()
                .map(|tool_call| {
                    json!({
                        "id": tool_call.call_id,
                        "type": "function",
                        "function": {
                            "name": tool_call.name,
                            "arguments": tool_call.arguments,
                        }
                    })
                })
                .collect(),
        ),
    );
    Value::Object(message)
}

fn build_chat_tool_message(output: Value) -> Option<Value> {
    let call_id = output.get("call_id")?.as_str()?;
    let content = output.get("output")?.as_str()?;
    Some(json!({
        "role": "tool",
        "tool_call_id": call_id,
        "content": content,
    }))
}

fn extract_responses_tool_calls(payload: &Value) -> Vec<PendingToolCall> {
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

fn extract_chat_tool_calls(payload: &Value) -> Vec<PendingToolCall> {
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

fn supports_compat_top_k(base_url: &str) -> bool {
    !base_url
        .trim()
        .to_ascii_lowercase()
        .contains("api.openai.com")
}

fn extract_embedded_upstream_error(payload: &Value) -> Option<(u16, String)> {
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

fn infer_embedded_upstream_status(code: Option<&Value>, message: &str) -> u16 {
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

fn normalize_openai_tool_parameters(parameters: &Value, strict: bool) -> Value {
    let mut normalized = normalize_openai_schema_shape(parameters);
    if strict {
        enforce_strict_openai_schema(&mut normalized);
    }
    normalized
}

fn normalize_openai_schema_shape(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut normalized = Map::new();
            for (key, value) in map {
                normalized.insert(key.clone(), normalize_openai_schema_shape(value));
            }
            if normalized.get("type").and_then(Value::as_str) == Some("array")
                && !normalized.contains_key("items")
            {
                normalized.insert("items".to_string(), json!({ "type": "string" }));
            }
            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(normalize_openai_schema_shape)
                .collect::<Vec<_>>(),
        ),
        _ => schema.clone(),
    }
}

fn enforce_strict_openai_schema(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };

    if let Some(any_of) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
        for branch in any_of {
            enforce_strict_openai_schema(branch);
        }
        return;
    }

    if let Some(items) = object.get_mut("items") {
        enforce_strict_openai_schema(items);
    }

    if object.get("type").and_then(Value::as_str) != Some("object") {
        return;
    }

    object
        .entry("additionalProperties".to_string())
        .or_insert_with(|| Value::Bool(false));

    let mut property_keys = Vec::new();
    if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
        property_keys = properties.keys().cloned().collect::<Vec<_>>();
        for key in &property_keys {
            if let Some(property) = properties.get_mut(key) {
                make_schema_nullable(property);
                enforce_strict_openai_schema(property);
            }
        }
    }

    if property_keys.is_empty() {
        object
            .entry("required".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        return;
    }

    let existing_required = object
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    let required = property_keys
        .iter()
        .map(|key| Value::String(key.clone()))
        .collect::<Vec<_>>();
    object.insert("required".to_string(), Value::Array(required));

    if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
        for key in &property_keys {
            if existing_required.contains(key) {
                continue;
            }
            if let Some(property) = properties.get_mut(key) {
                make_schema_nullable(property);
            }
        }
    }
}

fn make_schema_nullable(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };

    if let Some(any_of) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
        if any_of
            .iter()
            .any(|branch| branch.get("type").and_then(Value::as_str) == Some("null"))
        {
            return;
        }
        any_of.push(json!({ "type": "null" }));
        return;
    }

    match object.get_mut("type") {
        Some(Value::String(kind)) if kind != "null" => {
            let original = kind.clone();
            object.insert(
                "type".to_string(),
                Value::Array(vec![
                    Value::String(original),
                    Value::String("null".to_string()),
                ]),
            );
        }
        Some(Value::Array(items)) => {
            if !items.iter().any(|item| item.as_str() == Some("null")) {
                items.push(Value::String("null".to_string()));
            }
        }
        _ => {}
    }
}

fn llm_endpoint(base_url: &str, path_suffix: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.to_ascii_lowercase().ends_with("/v1") {
        format!("{base}/{path_suffix}")
    } else {
        format!("{base}/v1/{path_suffix}")
    }
}

fn log_outbound_llm_request(request_kind: &str, endpoint: &str, request: &Value) {
    let rendered = serde_json::to_string_pretty(request)
        .unwrap_or_else(|err| format!("<failed to serialize request body: {err}>"));
    emit_console_log(
        LogLevel::Debug,
        "llm.request",
        format!("POST {endpoint} ({request_kind})\n{rendered}"),
    );
}

fn should_fallback_to_chat_completions(status: u16, detail: &str) -> bool {
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
            return Some(text.trim().to_string()).filter(|value| !value.is_empty());
        }

        if let Some(parts) = chat_content.as_array() {
            let mut fragments = Vec::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        fragments.push(trimmed.to_string());
                    }
                }
            }
            if !fragments.is_empty() {
                return Some(fragments.join("\n"));
            }
        }
    }

    None
}

fn truncate_text(raw: &str, max_chars: usize) -> String {
    let mut iter = raw.chars();
    let preview: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liteyukibot_core::observability::set_console_log_output_enabled;
    use liteyukibot_core::recent_buffered_logs;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn log_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => unsafe {
                    std::env::set_var(self.key, value);
                },
                None => unsafe {
                    std::env::remove_var(self.key);
                },
            }
        }
    }

    fn spawn_mock_http_server(raw_responses: Vec<String>) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener.local_addr().expect("listener addr should exist");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured_requests = Arc::clone(&requests);

        thread::spawn(move || {
            for raw_response in raw_responses {
                let (mut socket, _) = listener.accept().expect("connection should be accepted");
                let mut request = Vec::new();
                let mut header_buf = [0_u8; 4096];
                loop {
                    let read = socket
                        .read(&mut header_buf)
                        .expect("request should be readable");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&header_buf[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }

                let header_end = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|index| index + 4)
                    .expect("request should have header separator");
                let header_text = String::from_utf8_lossy(&request[..header_end]);
                let content_length = header_text
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while request.len() < header_end + content_length {
                    let read = socket
                        .read(&mut header_buf)
                        .expect("body should be readable");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&header_buf[..read]);
                }
                captured_requests
                    .lock()
                    .expect("request capture lock should be available")
                    .push(String::from_utf8_lossy(&request).to_string());

                socket
                    .write_all(raw_response.as_bytes())
                    .expect("response should be writable");
            }
        });

        (addr, requests)
    }

    fn http_json_response(body: Value) -> String {
        let body = body.to_string();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    fn http_sse_response(events: &[Value]) -> String {
        let body = events
            .iter()
            .map(|event| format!("data: {}\n\n", event))
            .collect::<String>();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    fn request_body_json(raw_request: &str) -> Value {
        let body = raw_request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("request should contain header separator");
        serde_json::from_str(body).expect("request body should be valid json")
    }

    #[derive(Clone)]
    struct TestConfig {
        base_url: String,
        stream: bool,
        temperature: Option<f32>,
        top_p: Option<f32>,
        top_k: Option<u32>,
        frequency_penalty: Option<f32>,
        presence_penalty: Option<f32>,
    }

    impl OpenAiRuntimeConfig for TestConfig {
        fn base_url(&self) -> &str {
            self.base_url.as_str()
        }

        fn model(&self) -> &str {
            "gpt-5.2"
        }

        fn timeout_ms(&self) -> u64 {
            2_000
        }

        fn system_prompt(&self) -> Option<&str> {
            Some("system prompt")
        }

        fn stream(&self) -> bool {
            self.stream
        }

        fn temperature(&self) -> Option<f32> {
            self.temperature
        }

        fn top_p(&self) -> Option<f32> {
            self.top_p
        }

        fn top_k(&self) -> Option<u32> {
            self.top_k
        }

        fn frequency_penalty(&self) -> Option<f32> {
            self.frequency_penalty
        }

        fn presence_penalty(&self) -> Option<f32> {
            self.presence_penalty
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn responses_stream_emits_text_deltas_and_final_text() {
        let events = vec![
            json!({
                "type": "response.output_text.delta",
                "delta": "hel",
            }),
            json!({
                "type": "response.output_text.delta",
                "delta": "lo",
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "output_text": "hello"
                }
            }),
        ];
        let (addr, requests) = spawn_mock_http_server(vec![http_sse_response(&events)]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: true,
            temperature: Some(0.3),
            top_p: Some(0.9),
            top_k: Some(40),
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");

        let mut seen = Vec::new();
        let completion = client
            .generate_with_events("hello", &mut |event| seen.push(event))
            .await
            .expect("streaming call should succeed");

        assert_eq!(completion.text, "hello");
        assert_eq!(
            seen,
            vec![
                LlmStreamEvent::TextDelta("hel".to_string()),
                LlmStreamEvent::TextDelta("lo".to_string()),
            ]
        );

        let captured = requests.lock().expect("request capture should lock");
        assert_eq!(captured.len(), 1);
        let body = request_body_json(&captured[0]);
        assert_eq!(body.get("stream").and_then(Value::as_bool), Some(true));
        assert_eq!(
            body.get("instructions").and_then(Value::as_str),
            Some("system prompt")
        );
        assert!(
            body.get("temperature")
                .and_then(Value::as_f64)
                .is_some_and(|value| (value - 0.3_f64).abs() < 1e-6)
        );
        assert!(
            body.get("top_p")
                .and_then(Value::as_f64)
                .is_some_and(|value| (value - 0.9_f64).abs() < 1e-6)
        );
        assert_eq!(body.get("top_k").and_then(Value::as_u64), Some(40));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tool_call_loop_submits_function_call_output() {
        let initial_response = json!({
            "id": "resp_1",
            "output": [
                {
                    "id": "item_1",
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "lookup_weather",
                    "arguments": "{\"city\":\"Paris\"}"
                }
            ]
        });
        let final_response = json!({
            "id": "resp_2",
            "output_text": "Sunny in Paris"
        });
        let (addr, requests) = spawn_mock_http_server(vec![
            http_json_response(initial_response),
            http_json_response(final_response),
        ]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");
        let tool = LlmFunctionTool::new(
            "lookup_weather",
            json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            }),
            |arguments| async move {
                assert_eq!(arguments["city"], "Paris");
                Ok(LlmToolOutput::Text("Sunny in Paris".to_string()))
            },
        )
        .with_description("Look up the weather");

        let completion = client
            .complete("weather", &[tool], None)
            .await
            .expect("tool loop should succeed");

        assert_eq!(completion.text, "Sunny in Paris");
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].call_id, "call_1");
        assert_eq!(completion.tool_calls[0].name, "lookup_weather");
        assert_eq!(completion.tool_calls[0].output, "Sunny in Paris");

        let captured = requests.lock().expect("request capture should lock");
        assert_eq!(captured.len(), 2);
        assert!(captured[0].contains("\"tools\":["));
        assert!(captured[1].contains("\"previous_response_id\":\"resp_1\""));
        assert!(captured[1].contains("\"type\":\"function_call_output\""));
        assert!(captured[1].contains("\"call_id\":\"call_1\""));
        assert!(captured[1].contains("\"output\":\"Sunny in Paris\""));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn complete_with_chat_completions_uses_chat_endpoint_and_returns_text() {
        let response = json!({
            "id": "chatcmpl_1",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "hello from chat completions"
                    }
                }
            ]
        });
        let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: false,
            temperature: Some(0.4),
            top_p: Some(0.85),
            top_k: Some(32),
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");

        let completion = client
            .complete_with_chat_completions("hello", &[], None)
            .await
            .expect("chat completions request should succeed");
        assert_eq!(completion.text, "hello from chat completions");
        assert!(completion.tool_calls.is_empty());

        let captured = requests.lock().expect("request capture should lock");
        assert_eq!(captured.len(), 1);
        assert!(captured[0].contains("POST /v1/chat/completions HTTP/1.1"));
        let body = request_body_json(&captured[0]);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "system prompt");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "hello");
        assert!(
            body.get("temperature")
                .and_then(Value::as_f64)
                .is_some_and(|value| (value - 0.4_f64).abs() < 1e-6)
        );
        assert!(
            body.get("top_p")
                .and_then(Value::as_f64)
                .is_some_and(|value| (value - 0.85_f64).abs() < 1e-6)
        );
        assert_eq!(body.get("top_k").and_then(Value::as_u64), Some(32));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn complete_with_chat_messages_submits_tool_outputs() {
        let initial_response = json!({
            "id": "chatcmpl_1",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "lookup_weather",
                                    "arguments": "{\"city\":\"Paris\"}"
                                }
                            }
                        ]
                    }
                }
            ]
        });
        let final_response = json!({
            "id": "chatcmpl_2",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "Sunny in Paris"
                    }
                }
            ]
        });
        let (addr, requests) = spawn_mock_http_server(vec![
            http_json_response(initial_response),
            http_json_response(final_response),
        ]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");
        let tool = LlmFunctionTool::new(
            "lookup_weather",
            json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            }),
            |arguments| async move {
                assert_eq!(arguments["city"], "Paris");
                Ok(LlmToolOutput::Text("Sunny in Paris".to_string()))
            },
        )
        .with_description("Look up the weather");

        let completion = client
            .complete_with_chat_messages(
                vec![json!({
                    "role": "user",
                    "content": "weather"
                })],
                &[tool],
                None,
            )
            .await
            .expect("chat tool loop should succeed");

        assert_eq!(completion.text, "Sunny in Paris");
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].call_id, "call_1");

        let captured = requests.lock().expect("request capture should lock");
        assert_eq!(captured.len(), 2);
        assert!(captured[0].contains("POST /v1/chat/completions HTTP/1.1"));
        assert!(captured[1].contains("POST /v1/chat/completions HTTP/1.1"));
        let first_body = request_body_json(&captured[0]);
        let second_body = request_body_json(&captured[1]);
        assert_eq!(first_body["messages"][0]["role"], "user");
        assert_eq!(first_body["messages"][0]["content"], "weather");
        assert_eq!(second_body["messages"][1]["role"], "assistant");
        assert_eq!(second_body["messages"][1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(second_body["messages"][2]["role"], "tool");
        assert_eq!(second_body["messages"][2]["tool_call_id"], "call_1");
        assert_eq!(second_body["messages"][2]["content"], "Sunny in Paris");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn openai_requests_omit_compat_top_k() {
        let response = json!({
            "id": "resp_1",
            "output_text": "hello"
        });
        let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
        let config = TestConfig {
            base_url: format!("http://{addr}/proxy/api.openai.com"),
            stream: false,
            temperature: Some(0.2),
            top_p: Some(0.8),
            top_k: Some(64),
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");

        let output = client
            .generate("hello")
            .await
            .expect("request should succeed");
        assert_eq!(output, "hello");

        let captured = requests.lock().expect("request capture should lock");
        assert_eq!(captured.len(), 1);
        let body = request_body_json(&captured[0]);
        assert!(body.get("top_k").is_none());
        assert!(
            body.get("temperature")
                .and_then(Value::as_f64)
                .is_some_and(|value| (value - 0.2_f64).abs() < 1e-6)
        );
        assert!(
            body.get("top_p")
                .and_then(Value::as_f64)
                .is_some_and(|value| (value - 0.8_f64).abs() < 1e-6)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_debug_log_includes_json_body_before_send() {
        let _lock = log_test_lock()
            .lock()
            .expect("log test lock should not be poisoned");
        let _level_guard = EnvVarGuard::set("LY_LOG_LEVEL", "debug");
        let previous_console = set_console_log_output_enabled(false);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let prompt = format!("hello-debug-{unique}");
        let response = json!({
            "id": "resp_1",
            "output_text": "ok"
        });
        let (addr, _) = spawn_mock_http_server(vec![http_json_response(response)]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: false,
            temperature: Some(0.2),
            top_p: Some(0.8),
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");

        let output = client
            .generate(prompt.as_str())
            .await
            .expect("request should work");
        assert_eq!(output, "ok");

        assert!(recent_buffered_logs(200).iter().any(|entry| {
            entry.module == "llm.request"
                && entry.level == "DEBUG"
                && entry.message.contains("/responses")
                && entry.message.contains(prompt.as_str())
        }));

        set_console_log_output_enabled(previous_console);
    }

    #[test]
    fn extract_output_text_prefers_direct_output_text() {
        let payload = serde_json::json!({
            "output_text": "hello world"
        });
        assert_eq!(
            extract_output_text(&payload).as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn extract_output_text_reads_responses_output_content() {
        let payload = serde_json::json!({
            "output": [
                {
                    "content": [
                        { "type": "output_text", "text": "first" },
                        { "type": "output_text", "text": "second" }
                    ]
                }
            ]
        });
        assert_eq!(
            extract_output_text(&payload).as_deref(),
            Some("first\nsecond")
        );
    }

    #[test]
    fn extract_output_text_falls_back_to_chat_completions_shape() {
        let payload = serde_json::json!({
            "choices": [
                { "message": { "content": "fallback text" } }
            ]
        });
        assert_eq!(
            extract_output_text(&payload).as_deref(),
            Some("fallback text")
        );
    }

    #[test]
    fn llm_endpoint_adds_v1_when_missing() {
        assert_eq!(
            llm_endpoint("https://api.openai.com", "responses"),
            "https://api.openai.com/v1/responses"
        );
    }

    #[test]
    fn llm_endpoint_keeps_existing_v1() {
        assert_eq!(
            llm_endpoint("https://tokenflux.dev/v1", "responses"),
            "https://tokenflux.dev/v1/responses"
        );
    }

    #[test]
    fn fallback_triggered_by_instruction_required_error() {
        assert!(should_fallback_to_chat_completions(
            400,
            r#"{"detail":"Instructions are required"}"#
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn strict_tool_schema_marks_optional_fields_nullable_and_required() {
        let response = json!({
            "id": "chatcmpl_1",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "ok"
                    }
                }
            ]
        });
        let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");
        let tool = LlmFunctionTool::new(
            "workspace_list_files",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "max_depth": { "type": "integer" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            |_arguments| async move { Ok(LlmToolOutput::Text("[]".to_string())) },
        );

        let completion = client
            .complete_with_chat_completions("hello", &[tool], None)
            .await
            .expect("chat completions request should succeed");
        assert_eq!(completion.text, "ok");

        let captured = requests.lock().expect("request capture should lock");
        let body = request_body_json(&captured[0]);
        let parameters = &body["tools"][0]["function"]["parameters"];
        let mut required = parameters["required"]
            .as_array()
            .expect("required should be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        required.sort_unstable();
        assert_eq!(required, vec!["max_depth", "path"]);
        assert_eq!(parameters["additionalProperties"], Value::Bool(false));
        assert_eq!(
            parameters["properties"]["path"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(
            parameters["properties"]["max_depth"]["type"],
            json!(["integer", "null"])
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn custom_openai_endpoint_keeps_reasoning_when_tools_are_present() {
        let response = json!({
            "id": "chatcmpl_1",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "ok"
                    }
                }
            ]
        });
        let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
        let config = TestConfig {
            base_url: format!("http://{addr}/proxy/openai"),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let mut client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");
        client.reasoning_effort = Some("medium".to_string());
        let tool = LlmFunctionTool::new(
            "lookup_weather",
            json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            }),
            |_arguments| async move { Ok(LlmToolOutput::Text("ok".to_string())) },
        );

        client
            .complete_with_chat_completions("hello", &[tool], None)
            .await
            .expect("chat request should succeed");

        let captured = requests.lock().expect("request capture should lock");
        let body = request_body_json(&captured[0]);
        assert_eq!(body["reasoning"]["effort"], "medium");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn success_status_json_error_is_reported_as_upstream_failure() {
        let response = json!({
            "error": {
                "message": "Copilot API error: Bad Request",
                "code": 400
            }
        });
        let (addr, _) = spawn_mock_http_server(vec![http_json_response(response)]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");

        let err = client
            .complete_with_chat_completions("hello", &[], None)
            .await
            .expect_err("embedded error should fail the request");
        match err {
            LlmClientError::Upstream { status, detail } => {
                assert_eq!(status, 400);
                assert!(detail.contains("Bad Request"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn embedded_instruction_error_without_numeric_code_falls_back_to_chat() {
        let responses_error = json!({
            "error": {
                "message": "Instructions are required"
            }
        });
        let chat_response = json!({
            "id": "chatcmpl_1",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "fallback ok"
                    }
                }
            ]
        });
        let (addr, requests) = spawn_mock_http_server(vec![
            http_json_response(responses_error),
            http_json_response(chat_response),
        ]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");

        let completion = client
            .complete("hello", &[], None)
            .await
            .expect("instruction error should fall back to chat");
        assert_eq!(completion.text, "fallback ok");

        let captured = requests.lock().expect("request capture should lock");
        assert_eq!(captured.len(), 2);
        assert!(captured[0].contains("POST /v1/responses HTTP/1.1"));
        assert!(captured[1].contains("POST /v1/chat/completions HTTP/1.1"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn responses_stream_instruction_error_is_classified_as_bad_request() {
        let events = vec![json!({
            "type": "response.failed",
            "response": {
                "error": {
                    "message": "Instructions are required"
                }
            }
        })];
        let (addr, _) = spawn_mock_http_server(vec![http_sse_response(&events)]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: true,
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");
        let request = client.build_responses_request(
            &ResponsesTurnInput::Initial {
                input: Value::String("hello".to_string()),
            },
            &[],
            true,
        );
        let mut dispatcher = EventDispatcher::new(None);

        let err = client
            .send_responses_stream(
                llm_endpoint(client.base_url.as_str(), "responses").as_str(),
                request,
                &mut dispatcher,
            )
            .await
            .expect_err("response.failed event should fail the stream");
        match err {
            LlmClientError::Upstream { status, detail } => {
                assert_eq!(status, 400);
                assert!(detail.contains("Instructions are required"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn chat_stream_reports_embedded_sse_errors() {
        let events = vec![json!({
            "error": {
                "message": "Copilot API error: Bad Request",
                "code": 400
            }
        })];
        let (addr, _) = spawn_mock_http_server(vec![http_sse_response(&events)]);
        let config = TestConfig {
            base_url: format!("http://{addr}"),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
            .expect("client should build");
        let mut seen = Vec::new();

        let err = client
            .complete_with_chat_completions("hello", &[], Some(&mut |event| seen.push(event)))
            .await
            .expect_err("embedded SSE error should fail the request");
        assert!(seen.is_empty());
        match err {
            LlmClientError::Upstream { status, detail } => {
                assert_eq!(status, 400);
                assert!(detail.contains("Bad Request"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
