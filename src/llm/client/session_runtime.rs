use serde_json::Value;

use super::event_dispatch::EventDispatcher;
use super::protocol::{ChatHistory, PendingToolCall, ProviderTurn, ResponsesTurnInput};
use super::{
    LlmClientError, LlmCompletion, LlmEventSink, LlmExecutedToolCall, LlmFunctionTool,
    LlmStreamEvent, OpenAiResponsesClient, append_turn_text, should_fallback_to_chat_completions,
};

impl OpenAiResponsesClient {
    pub async fn generate(&self, prompt: &str) -> Result<String, LlmClientError> {
        let sink: Option<&mut dyn LlmEventSink> = None;
        self.complete(prompt, &[], sink)
            .await
            .map(|completion| completion.text)
    }

    // 外部调用
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
        let initial = self
            .start_session(prompt, input, tools, stream, &mut dispatcher)
            .await?;
        self.run_session_loop(
            initial.session,
            initial.turn,
            tools,
            stream,
            &mut dispatcher,
        )
        .await
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
        let history = ChatHistory::from_prompt(self.system_prompt.as_deref(), Some(prompt));
        self.complete_with_chat_history(history, tools, sink).await
    }

    // 外部调用
    #[allow(dead_code)]
    pub async fn complete_with_chat_messages(
        &self,
        history: Vec<Value>,
        tools: &[LlmFunctionTool],
        sink: Option<&mut dyn LlmEventSink>,
    ) -> Result<LlmCompletion, LlmClientError> {
        let history = ChatHistory::from_messages(history);
        self.complete_with_chat_history(history, tools, sink).await
    }

    pub(super) async fn start_session(
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
                let history = ChatHistory::from_prompt(
                    self.system_prompt.as_deref(),
                    Some(prompt_fallback.as_str()),
                );
                let turn = self
                    .send_chat_turn(&history, tools, stream, dispatcher)
                    .await?;
                Ok(ProviderStart {
                    session: ProviderSession::Chat { history },
                    turn,
                })
            }
            Err(err) => Err(err),
        }
    }

    async fn run_session_loop(
        &self,
        mut session: ProviderSession,
        mut turn: ProviderTurn,
        tools: &[LlmFunctionTool],
        stream: bool,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<LlmCompletion, LlmClientError> {
        let mut executed_tools = Vec::new();
        let mut accumulated_text = String::new();

        loop {
            append_turn_text(&mut accumulated_text, turn.text.as_str());

            if turn.tool_calls.is_empty() {
                return Ok(LlmCompletion {
                    text: accumulated_text,
                    tool_calls: executed_tools,
                });
            }

            let executed = execute_tool_calls(&turn.tool_calls, tools, dispatcher).await?;
            turn = session
                .continue_with_tool_outputs(
                    self,
                    &turn,
                    executed.as_slice(),
                    tools,
                    stream,
                    dispatcher,
                )
                .await?;
            executed_tools.extend(executed);
        }
    }

    async fn complete_with_chat_history(
        &self,
        history: ChatHistory,
        tools: &[LlmFunctionTool],
        sink: Option<&mut dyn LlmEventSink>,
    ) -> Result<LlmCompletion, LlmClientError> {
        let mut dispatcher = EventDispatcher::new(sink);
        let stream = self.stream || dispatcher.has_sink();
        let turn = self
            .send_chat_turn(&history, tools, stream, &mut dispatcher)
            .await?;
        let session = ProviderSession::Chat { history };
        self.run_session_loop(session, turn, tools, stream, &mut dispatcher)
            .await
    }
}

#[derive(Debug, Clone)]
pub(super) enum ProviderSession {
    Responses,
    Chat { history: ChatHistory },
}

impl ProviderSession {
    async fn continue_with_tool_outputs(
        &mut self,
        client: &OpenAiResponsesClient,
        previous_turn: &ProviderTurn,
        executed_tools: &[LlmExecutedToolCall],
        tools: &[LlmFunctionTool],
        stream: bool,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        match self {
            Self::Responses => {
                client
                    .send_responses_turn(
                        previous_turn.into_responses_turn_input(executed_tools)?,
                        tools,
                        stream,
                        dispatcher,
                    )
                    .await
            }
            Self::Chat { history } => {
                previous_turn.extend_chat_history(history, executed_tools)?;
                client
                    .send_chat_turn(history, tools, stream, dispatcher)
                    .await
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ProviderStart {
    pub(super) session: ProviderSession,
    pub(super) turn: ProviderTurn,
}

async fn execute_tool_calls(
    tool_calls: &[PendingToolCall],
    tools: &[LlmFunctionTool],
    dispatcher: &mut EventDispatcher<'_>,
) -> Result<Vec<LlmExecutedToolCall>, LlmClientError> {
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
    }

    Ok(executed)
}
