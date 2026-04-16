use std::time::Duration;

use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

use crate::app_config::LlmRuntimeConfig;

const OUTPUT_TRUNCATE_LIMIT: usize = 320;

#[derive(Debug, Clone)]
pub(crate) struct OpenAiResponsesClient {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    system_prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum LlmClientError {
    NotConfigured(String),
    Http(String),
    Upstream { status: u16, detail: String },
    InvalidResponse(String),
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
        }
    }
}

impl std::error::Error for LlmClientError {}

impl OpenAiResponsesClient {
    pub(crate) fn from_runtime_with_api_key(
        config: &LlmRuntimeConfig,
        api_key: &str,
    ) -> Result<Self, LlmClientError> {
        let api_key = api_key.trim().to_string();
        if api_key.is_empty() {
            return Err(LlmClientError::NotConfigured(
                "missing api key for current request".to_string(),
            ));
        }
        let client = Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms.max(10)))
            .build()
            .map_err(|err| LlmClientError::NotConfigured(format!("reqwest init failed: {err}")))?;

        Ok(Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            api_key,
            model: config.model.clone(),
            system_prompt: config.system_prompt.clone(),
        })
    }

    pub(crate) async fn generate(&self, prompt: &str) -> Result<String, LlmClientError> {
        let responses_request = ResponsesRequest::from_input(
            self.model.clone(),
            self.system_prompt.clone(),
            prompt.to_string(),
        );
        let responses_endpoint = llm_endpoint(self.base_url.as_str(), "responses");

        match self
            .send_and_extract(responses_endpoint.as_str(), &responses_request)
            .await
        {
            Ok(output) => Ok(output),
            Err(LlmClientError::Upstream { status, detail })
                if should_fallback_to_chat_completions(status, detail.as_str()) =>
            {
                let fallback_request = ChatCompletionsRequest::from_input(
                    self.model.clone(),
                    self.system_prompt.clone(),
                    prompt.to_string(),
                );
                let fallback_endpoint = llm_endpoint(self.base_url.as_str(), "chat/completions");
                self.send_and_extract(fallback_endpoint.as_str(), &fallback_request)
                    .await
            }
            Err(err) => Err(err),
        }
    }

    async fn send_and_extract<T: Serialize>(
        &self,
        endpoint: &str,
        request: &T,
    ) -> Result<String, LlmClientError> {
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.api_key)
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

        let payload: Value = serde_json::from_str(&body)
            .map_err(|err| LlmClientError::InvalidResponse(err.to_string()))?;
        extract_output_text(&payload).ok_or_else(|| {
            LlmClientError::InvalidResponse("no textual output in response payload".to_string())
        })
    }
}

#[derive(Debug, Serialize)]
struct ResponsesRequest {
    model: String,
    input: Vec<ResponsesInputMessage>,
}

impl ResponsesRequest {
    fn from_input(model: String, system_prompt: Option<String>, user_prompt: String) -> Self {
        let mut input = Vec::new();
        if let Some(system_prompt) = system_prompt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
        {
            input.push(ResponsesInputMessage::new("system", system_prompt));
        }

        // Keep at least one user message item to match responses providers that
        // require `input` to be a list of message items.
        let user_prompt = user_prompt.trim().to_string();
        input.push(ResponsesInputMessage::new("user", user_prompt));
        Self { model, input }
    }
}

#[derive(Debug, Serialize)]
struct ResponsesInputMessage {
    role: String,
    content: Vec<ResponsesInputText>,
}

impl ResponsesInputMessage {
    fn new(role: &str, text: String) -> Self {
        Self {
            role: role.to_string(),
            content: vec![ResponsesInputText::new(text)],
        }
    }
}

#[derive(Debug, Serialize)]
struct ResponsesInputText {
    #[serde(rename = "type")]
    content_type: &'static str,
    text: String,
}

impl ResponsesInputText {
    fn new(text: String) -> Self {
        Self {
            content_type: "input_text",
            text,
        }
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

impl ChatCompletionsRequest {
    fn from_input(model: String, system_prompt: Option<String>, user_prompt: String) -> Self {
        let mut messages = Vec::new();
        if let Some(system_prompt) = system_prompt {
            messages.push(ChatMessage::new("system", system_prompt));
        }
        messages.push(ChatMessage::new("user", user_prompt));
        Self { model, messages }
    }
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

impl ChatMessage {
    fn new(role: &str, content: String) -> Self {
        Self {
            role: role.to_string(),
            content,
        }
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
    use super::ResponsesRequest;
    use super::{extract_output_text, llm_endpoint, should_fallback_to_chat_completions};

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

    #[test]
    fn responses_request_serializes_input_as_message_list() {
        let request = ResponsesRequest::from_input(
            "gpt-5.2".to_string(),
            Some("system prompt".to_string()),
            "hello".to_string(),
        );
        let json = serde_json::to_value(request).expect("responses request should serialize");

        let input = json
            .get("input")
            .and_then(serde_json::Value::as_array)
            .expect("input should be array");
        assert_eq!(input.len(), 2);
        assert_eq!(
            input[0].get("role").and_then(serde_json::Value::as_str),
            Some("system")
        );
        assert_eq!(
            input[1].get("role").and_then(serde_json::Value::as_str),
            Some("user")
        );
        assert_eq!(
            input[1]
                .pointer("/content/0/type")
                .and_then(serde_json::Value::as_str),
            Some("input_text")
        );
    }
}
