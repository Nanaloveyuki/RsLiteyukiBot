use futures_util::StreamExt;
use reqwest::RequestBuilder;
use serde_json::Value;

use liteyukibot_core::{LogLevel, SseParser, emit_console_log};

use super::event_dispatch::EventDispatcher;
use super::protocol::{ChatHistory, ProviderTurn, ResponsesTurnInput};
use super::request_encoding::{build_chat_request, build_responses_request, llm_endpoint};
use super::response_parsing::{
    ChatStreamState, ResponsesStreamState, build_chat_turn_from_payload,
    build_responses_turn_from_payload, extract_embedded_upstream_error, truncate_text,
};
use super::{LlmClientError, LlmFunctionTool, OUTPUT_TRUNCATE_LIMIT, OpenAiResponsesClient};

impl OpenAiResponsesClient {
    pub(super) async fn send_responses_turn(
        &self,
        input: ResponsesTurnInput,
        tools: &[LlmFunctionTool],
        stream: bool,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let request = build_responses_request(self, &input, tools, stream);
        let endpoint = llm_endpoint(self.base_url.as_str(), "responses");
        if stream {
            self.send_responses_stream(endpoint.as_str(), request, dispatcher)
                .await
        } else {
            self.send_responses_json(endpoint.as_str(), request, dispatcher)
                .await
        }
    }

    pub(super) async fn send_chat_turn(
        &self,
        history: &ChatHistory,
        tools: &[LlmFunctionTool],
        stream: bool,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let request = build_chat_request(self, history, tools, stream);
        let endpoint = llm_endpoint(self.base_url.as_str(), "chat/completions");
        if stream {
            self.send_chat_stream(endpoint.as_str(), request, dispatcher)
                .await
        } else {
            self.send_chat_json(endpoint.as_str(), request, dispatcher)
                .await
        }
    }

    pub(super) async fn send_responses_json(
        &self,
        endpoint: &str,
        request: Value,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let payload = self.send_json(endpoint, &request).await?;
        Ok(build_responses_turn_from_payload(&payload, dispatcher))
    }

    pub(super) async fn send_chat_json(
        &self,
        endpoint: &str,
        request: Value,
        dispatcher: &mut EventDispatcher<'_>,
    ) -> Result<ProviderTurn, LlmClientError> {
        let payload = self.send_json(endpoint, &request).await?;
        Ok(build_chat_turn_from_payload(&payload, dispatcher))
    }

    pub(super) async fn send_responses_stream(
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
        let mut stream_state = ResponsesStreamState::default();

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
                stream_state.apply_event(&payload, dispatcher)?;
            }
        }

        stream_state.finish(dispatcher)
    }

    pub(super) async fn send_chat_stream(
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
        let mut stream_state = ChatStreamState::default();

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
                stream_state.apply_event(&payload, dispatcher)?;
            }
        }

        Ok(stream_state.finish(dispatcher))
    }

    pub(super) async fn send_json(
        &self,
        endpoint: &str,
        request: &Value,
    ) -> Result<Value, LlmClientError> {
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

    fn apply_default_headers(&self, mut request: RequestBuilder) -> RequestBuilder {
        for (name, value) in &self.default_headers {
            request = request.header(name, value);
        }
        request
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
