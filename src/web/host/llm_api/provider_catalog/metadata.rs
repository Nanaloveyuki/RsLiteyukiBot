use serde_json::{Value, json};

use crate::llm::service::detect_provider_id_from_base_url;

use super::{
    provider_catalog_label, provider_catalog_parameter_support, provider_catalog_sample_models,
};

pub(in super::super) fn normalize_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

pub(in super::super) fn detect_provider_id(base_url: &str) -> String {
    detect_provider_id_from_base_url(base_url)
}

pub(in super::super) fn provider_label(provider_id: &str) -> String {
    provider_catalog_label(provider_id).unwrap_or_else(|| "OpenAI Compatible".to_string())
}

pub(in super::super) fn current_provider_supports(
    provider_id: &str,
    base_url: &str,
    reasoning_options: &[String],
) -> Value {
    let mut supports = provider_catalog_parameter_support(provider_id)
        .map(runtime_supports_from_catalog)
        .unwrap_or_else(|| fallback_runtime_supports(base_url));
    if let Some(map) = supports.as_object_mut() {
        map.insert(
            "reasoningEffort".to_string(),
            Value::Bool(!reasoning_options.is_empty()),
        );
    }
    supports
}

pub(in super::super) fn model_options_for_provider(provider_id: &str) -> Vec<String> {
    provider_catalog_sample_models(provider_id)
}

pub(in super::super) fn reasoning_options_for_provider(
    provider_id: &str,
    model: &str,
) -> Vec<String> {
    if !matches!(provider_id, "openai" | "openai-compatible") {
        return Vec::new();
    }
    let model = model.to_ascii_lowercase();
    if model.starts_with("gpt-5") {
        return vec![
            "none".to_string(),
            "minimal".to_string(),
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ];
    }
    Vec::new()
}

fn runtime_supports_from_catalog(parameter_support: serde_json::Map<String, Value>) -> Value {
    json!({
        "streaming": support_state_enabled(parameter_support.get("streaming")),
        "temperature": support_state_enabled(parameter_support.get("temperature")),
        "topP": support_state_enabled(parameter_support.get("topP")),
        "topK": support_state_enabled(parameter_support.get("topK")),
        "frequencyPenalty": support_state_enabled(parameter_support.get("frequencyPenalty")),
        "presencePenalty": support_state_enabled(parameter_support.get("presencePenalty")),
        "reasoningEffort": false,
        "imageInput": support_state_enabled(parameter_support.get("imageInput")),
        "textFileInput": support_state_enabled(parameter_support.get("textFileInput")),
        "binaryFileInput": support_state_enabled(parameter_support.get("binaryFileInput")),
    })
}

fn fallback_runtime_supports(base_url: &str) -> Value {
    json!({
        "streaming": true,
        "temperature": true,
        "topP": true,
        "topK": !base_url.to_ascii_lowercase().contains("api.openai.com"),
        "frequencyPenalty": true,
        "presencePenalty": true,
        "reasoningEffort": false,
        "imageInput": false,
        "textFileInput": true,
        "binaryFileInput": false
    })
}

fn support_state_enabled(value: Option<&Value>) -> bool {
    matches!(
        value.and_then(Value::as_str),
        Some("supported" | "conditional" | "fixed")
    )
}
