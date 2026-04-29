use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use crate::llm::OpenAiRuntimeConfig;

#[derive(Debug, Clone)]
pub(super) enum ProviderAuth {
    Bearer,
    ApiKeyHeader(String),
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct WebLlmChatRequest {
    #[serde(default)]
    pub(super) message: String,
    #[serde(default)]
    pub(super) messages: Vec<WebLlmMessage>,
    #[serde(default)]
    pub(super) attachments: Vec<WebLlmAttachment>,
    #[serde(default, rename = "baseUrl")]
    pub(super) base_url: Option<String>,
    #[serde(default)]
    pub(super) model: Option<String>,
    #[serde(default, rename = "reasoningEffort")]
    pub(super) reasoning_effort: Option<String>,
    #[serde(default, rename = "temperature")]
    pub(super) temperature: Option<f32>,
    #[serde(default, rename = "topP")]
    pub(super) top_p: Option<f32>,
    #[serde(default, rename = "topK")]
    pub(super) top_k: Option<u32>,
    #[serde(default, rename = "frequencyPenalty")]
    pub(super) frequency_penalty: Option<f32>,
    #[serde(default, rename = "presencePenalty")]
    pub(super) presence_penalty: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct WebLlmMessage {
    #[serde(default)]
    pub(super) role: String,
    #[serde(default)]
    pub(super) content: String,
    #[serde(default)]
    pub(super) attachments: Vec<WebLlmAttachment>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub(super) struct WebLlmAttachment {
    #[serde(default)]
    pub(super) kind: String,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default, rename = "mediaType")]
    pub(super) media_type: Option<String>,
    #[serde(default)]
    pub(super) size: Option<u64>,
    #[serde(default, rename = "dataUrl")]
    pub(super) data_url: Option<String>,
    #[serde(default)]
    pub(super) text: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct WebLlmRuntimeConfig {
    pub(super) base_url: String,
    pub(super) model: String,
    pub(super) timeout_ms: u64,
    pub(super) system_prompt: Option<String>,
    pub(super) stream: bool,
    pub(super) temperature: Option<f32>,
    pub(super) top_p: Option<f32>,
    pub(super) top_k: Option<u32>,
    pub(super) frequency_penalty: Option<f32>,
    pub(super) presence_penalty: Option<f32>,
    pub(super) parallel_tool_calls: bool,
    pub(super) reasoning_effort: Option<String>,
    pub(super) default_headers: HashMap<String, String>,
}

impl OpenAiRuntimeConfig for WebLlmRuntimeConfig {
    fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    fn model(&self) -> &str {
        self.model.as_str()
    }

    fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
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

    fn parallel_tool_calls(&self) -> bool {
        self.parallel_tool_calls
    }

    fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    fn default_headers(&self) -> Option<&HashMap<String, String>> {
        Some(&self.default_headers)
    }
}

#[derive(Debug, Clone)]
pub(super) struct WebLlmChatExecution {
    pub(super) message: String,
    pub(super) model: String,
    pub(super) base_url: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct WebLlmManagerSaveRequest {
    #[serde(default)]
    pub(super) active_provider_id: Option<String>,
    #[serde(default)]
    pub(super) providers: Vec<WebLlmManagedProviderPayload>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct WebLlmPromptProfileSaveRequest {
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) soul: String,
    #[serde(default)]
    pub(super) active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct WebLlmPromptProfileNameRequest {
    #[serde(default)]
    pub(super) name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct WebLlmPromptProfilePreviewRequest {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) user_prompt: Option<String>,
    #[serde(default)]
    pub(super) system_prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct WebLlmProviderActionRequest {
    #[serde(default)]
    pub(super) provider: WebLlmManagedProviderPayload,
    #[serde(default)]
    pub(super) model_id: Option<String>,
    #[serde(default)]
    pub(super) test_all: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct WebLlmManagedProviderPayload {
    #[serde(default)]
    pub(super) id: String,
    #[serde(default)]
    pub(super) label: String,
    #[serde(default)]
    pub(super) provider_id: Option<String>,
    #[serde(default)]
    pub(super) base_url: String,
    #[serde(default)]
    pub(super) api_key: String,
    #[serde(default)]
    pub(super) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(super) headers: HashMap<String, String>,
    #[serde(default)]
    pub(super) models: Vec<WebLlmManagedModelPayload>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct WebLlmManagedModelPayload {
    #[serde(default)]
    pub(super) id: String,
    #[serde(default)]
    pub(super) enabled: bool,
}

#[derive(Debug, Clone)]
pub(super) struct PreparedProviderRequest {
    pub(super) endpoint: String,
    pub(super) auth: Option<ProviderAuth>,
    pub(super) api_key: Option<String>,
    pub(super) extra_headers: Vec<(String, String)>,
    pub(super) payload: Value,
}
