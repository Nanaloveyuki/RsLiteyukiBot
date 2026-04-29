use crate::app_config::LlmRuntimeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LlmProviderApiFamily {
    Responses,
    ChatCompletions,
    AnthropicMessages,
    GeminiGenerateContent,
}

pub(crate) fn resolve_runtime_provider_id(llm_config: &LlmRuntimeConfig) -> String {
    resolve_provider_id(
        Some(llm_config.provider.as_str()),
        Some(llm_config.base_url.as_str()),
    )
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn provider_uses_openai_runtime(provider_id: &str) -> bool {
    matches!(
        provider_api_family(provider_id),
        LlmProviderApiFamily::Responses | LlmProviderApiFamily::ChatCompletions
    )
}

pub(crate) fn provider_api_family(provider_id: &str) -> LlmProviderApiFamily {
    match provider_id {
        "openai" | "openai-compatible" => LlmProviderApiFamily::Responses,
        "openrouter" | "kimi" | "qwen" => LlmProviderApiFamily::ChatCompletions,
        "anthropic" => LlmProviderApiFamily::AnthropicMessages,
        "google-gemini" => LlmProviderApiFamily::GeminiGenerateContent,
        _ => LlmProviderApiFamily::Responses,
    }
}

pub(crate) fn resolve_provider_id(
    configured_provider: Option<&str>,
    base_url: Option<&str>,
) -> String {
    let configured = configured_provider.and_then(normalize_provider_id);
    let detected = base_url
        .map(detect_provider_id_from_base_url)
        .unwrap_or_else(|| "openai-compatible".to_string());

    match configured.as_deref() {
        Some("openai") => match detected.as_str() {
            "openrouter" | "kimi" | "qwen" => detected,
            _ => "openai".to_string(),
        },
        Some("openai-compatible") => match detected.as_str() {
            "openai" => "openai-compatible".to_string(),
            _ => detected,
        },
        Some(explicit @ ("openrouter" | "kimi" | "qwen" | "anthropic" | "google-gemini")) => {
            explicit.to_string()
        }
        Some(_) | None => detected,
    }
}

pub(crate) fn detect_provider_id_from_base_url(base_url: &str) -> String {
    let normalized = base_url.to_ascii_lowercase();
    if normalized.contains("api.openai.com") {
        "openai".to_string()
    } else if normalized.contains("openrouter.ai") {
        "openrouter".to_string()
    } else if normalized.contains("moonshot.ai") {
        "kimi".to_string()
    } else if normalized.contains("dashscope.aliyuncs.com")
        || normalized.contains("dashscope-intl.aliyuncs.com")
    {
        "qwen".to_string()
    } else if normalized.contains("generativelanguage.googleapis.com") {
        "google-gemini".to_string()
    } else if normalized.contains("anthropic.com") {
        "anthropic".to_string()
    } else {
        "openai-compatible".to_string()
    }
}

fn normalize_provider_id(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" => None,
        "openai" | "openai-compatible" | "openrouter" | "kimi" | "qwen" | "anthropic"
        | "google-gemini" => Some(normalized),
        _ => None,
    }
}

#[cfg(test)]
#[path = "provider_selection/tests.rs"]
mod tests;
