use super::*;

fn runtime_config(provider: &str, base_url: &str) -> LlmRuntimeConfig {
    LlmRuntimeConfig {
        enabled: true,
        stream: false,
        provider: provider.to_string(),
        base_url: base_url.to_string(),
        api_keys: vec!["sk-test".to_string()],
        headers: Default::default(),
        model: "gpt-5-mini".to_string(),
        timeout_ms: 20_000,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
        parallel_tool_calls: true,
        system_prompt: None,
        command_prefix: "/ask".to_string(),
    }
}

#[test]
// 必要测试
fn resolve_runtime_provider_id_prefers_detected_openai_family_from_base_url() {
    let config = runtime_config("openai", "https://openrouter.ai/api");
    assert_eq!(resolve_runtime_provider_id(&config), "openrouter");
}

#[test]
// 必要测试
fn provider_uses_openai_runtime_accepts_openai_style_providers() {
    assert!(provider_uses_openai_runtime("openai"));
    assert!(provider_uses_openai_runtime("openai-compatible"));
    assert!(provider_uses_openai_runtime("qwen"));
    assert!(!provider_uses_openai_runtime("anthropic"));
    assert!(!provider_uses_openai_runtime("google-gemini"));
}

#[test]
// 必要测试
fn resolve_provider_id_prefers_explicit_provider_for_custom_gateway() {
    assert_eq!(
        resolve_provider_id(
            Some("anthropic"),
            Some("https://llm-proxy.internal/company-gateway"),
        ),
        "anthropic"
    );
    assert_eq!(
        resolve_provider_id(
            Some("google-gemini"),
            Some("https://gateway.example/v1beta"),
        ),
        "google-gemini"
    );
}

#[test]
// 必要测试
fn resolve_provider_id_detects_qwen_international_base_urls() {
    assert_eq!(
        resolve_provider_id(
            Some("openai-compatible"),
            Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        ),
        "qwen"
    );
    assert_eq!(
        detect_provider_id_from_base_url("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        "qwen"
    );
}
