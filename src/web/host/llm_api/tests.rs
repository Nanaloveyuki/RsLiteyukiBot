use super::{
    ProviderAuth, WebLlmAttachment, WebLlmChatRequest, WebLlmManagedModelPayload,
    WebLlmManagedProviderPayload, WebLlmManagerSaveRequest, WebLlmMessage, WebLlmRuntimeConfig,
    build_anthropic_request, build_chat_completions_request_payload, build_gemini_request,
    build_openai_compatible_request_payload, build_openrouter_request, build_responses_input,
    compose_chat_fallback_prompt, current_provider_supports, detect_provider_id,
    discover_models_for_provider, merge_saved_and_discovered_models,
    normalize_reasoning_effort_input, normalize_single_provider_input, preview_headers_map,
    provider_catalog, reasoning_options_for_provider, redact_preview_payload,
    reject_non_post_method, resolve_active_provider_id, resolve_llm_enabled_write_path_for_web,
    resolve_models_to_test, route_llm_api, serialize_managed_provider,
    serialize_managed_provider_models, summarize_frontend_chat_request, validate_sampling_args,
};

use std::collections::HashMap;
use std::fs;

use serde_json::json;

use crate::app_config::{LlmManagedModelConfig, LlmManagedProviderConfig};
use crate::app_host::AppHostSnapshot;
use crate::web::host::{WebHostAsset, WebHostAssets, WebHostConfig, WebHostService};
use liteyukibot_core::test_support::{CurrentDirGuard, EnvVarGuard, process_state_lock};

#[test]
fn openai_reasoning_options_follow_model_family() {
    assert_eq!(
        reasoning_options_for_provider("openai", "gpt-5-mini"),
        vec![
            "none".to_string(),
            "minimal".to_string(),
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ]
    );
    assert_eq!(
        reasoning_options_for_provider("openai-compatible", "gpt-5-mini"),
        vec![
            "none".to_string(),
            "minimal".to_string(),
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ]
    );
    assert!(reasoning_options_for_provider("openai", "gpt-4.1").is_empty());
}

#[test]
fn responses_input_keeps_image_attachments_for_openai() {
    let request = WebLlmChatRequest {
        message: "describe this".to_string(),
        attachments: vec![WebLlmAttachment {
            kind: "image".to_string(),
            name: "clipboard.png".to_string(),
            media_type: Some("image/png".to_string()),
            size: Some(3),
            data_url: Some("data:image/png;base64,abc".to_string()),
            text: None,
        }],
        ..Default::default()
    };

    let input =
        build_responses_input(&request, "be concise", "openai").expect("input should build");
    let content = input[0]["content"]
        .as_array()
        .expect("responses input content should be an array");
    assert_eq!(content[0]["type"], "input_text");
    assert_eq!(content[1]["type"], "input_image");
    assert_eq!(content[1]["image_url"], "data:image/png;base64,abc");
}

#[test]
fn anthropic_request_embeds_base64_image_blocks() {
    let request = WebLlmChatRequest {
        messages: vec![WebLlmMessage {
            role: "user".to_string(),
            content: "look".to_string(),
            attachments: vec![WebLlmAttachment {
                kind: "image".to_string(),
                name: "clipboard.png".to_string(),
                media_type: Some("image/png".to_string()),
                size: None,
                data_url: Some("data:image/png;base64,abcd".to_string()),
                text: None,
            }],
        }],
        ..Default::default()
    };

    let payload = build_anthropic_request(
        &request,
        Some("system"),
        "persona",
        "claude-sonnet-4-20250514",
        Some(0.7),
        Some(0.95),
        Some(32),
        Some("medium"),
    )
    .expect("anthropic payload should build");

    assert_eq!(
        payload["system"],
        "system\n\nPrompt profile instruction:\npersona"
    );
    assert_eq!(
        payload["messages"][0]["content"][1]["source"]["type"],
        "base64"
    );
    assert_eq!(
        payload["messages"][0]["content"][1]["source"]["media_type"],
        "image/png"
    );
    assert_eq!(
        payload["messages"][0]["content"][1]["source"]["data"],
        "abcd"
    );
    assert_eq!(payload["thinking"]["budget_tokens"], 4096);
    assert!(payload.get("temperature").is_none());
    assert!(payload.get("top_k").is_none());
}

#[test]
fn gemini_request_uses_inline_data_parts() {
    let request = WebLlmChatRequest {
        messages: vec![WebLlmMessage {
            role: "user".to_string(),
            content: "".to_string(),
            attachments: vec![WebLlmAttachment {
                kind: "image".to_string(),
                name: "clipboard.png".to_string(),
                media_type: Some("image/jpeg".to_string()),
                size: None,
                data_url: Some("data:image/jpeg;base64,xyz".to_string()),
                text: None,
            }],
        }],
        ..Default::default()
    };

    let payload = build_gemini_request(
        &request,
        None,
        "",
        "gemini-2.5-flash",
        Some(0.3),
        Some(0.8),
        Some(20),
        Some("minimal"),
    )
    .expect("gemini payload should build");

    assert_eq!(
        payload["contents"][0]["parts"][0]["inline_data"]["mime_type"],
        "image/jpeg"
    );
    assert_eq!(
        payload["contents"][0]["parts"][0]["inline_data"]["data"],
        "xyz"
    );
    assert_eq!(
        payload["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        0
    );
}

#[test]
fn openrouter_request_uses_multimodal_messages() {
    let request = WebLlmChatRequest {
        messages: vec![WebLlmMessage {
            role: "user".to_string(),
            content: "describe".to_string(),
            attachments: vec![WebLlmAttachment {
                kind: "image".to_string(),
                name: "clipboard.png".to_string(),
                media_type: Some("image/png".to_string()),
                size: None,
                data_url: Some("data:image/png;base64,abc".to_string()),
                text: None,
            }],
        }],
        ..Default::default()
    };

    let payload = build_openrouter_request(
        &request,
        None,
        "persona",
        "openai/gpt-5",
        Some(0.5),
        Some(0.9),
        Some(0.2),
        Some(-0.3),
        Some("high"),
    )
    .expect("openrouter payload should build");

    assert_eq!(payload["messages"][0]["role"], "system");
    assert_eq!(payload["messages"][1]["content"][0]["type"], "text");
    assert_eq!(payload["messages"][1]["content"][1]["type"], "image_url");
    assert_eq!(
        payload["messages"][1]["content"][1]["image_url"]["url"],
        "data:image/png;base64,abc"
    );
    let frequency_penalty = payload["frequency_penalty"]
        .as_f64()
        .expect("frequency penalty should be numeric");
    let presence_penalty = payload["presence_penalty"]
        .as_f64()
        .expect("presence penalty should be numeric");
    assert!((frequency_penalty - 0.2).abs() < 1e-6);
    assert!((presence_penalty + 0.3).abs() < 1e-6);
    assert_eq!(payload["reasoning"]["effort"], "high");
}

#[test]
fn validate_sampling_args_accepts_penalty_range() {
    assert!(validate_sampling_args(Some(1.0), Some(0.9), Some(40), Some(-2.0), Some(2.0)).is_ok());
    assert_eq!(
        validate_sampling_args(None, None, None, Some(2.1), None).unwrap_err(),
        "frequencyPenalty should be within -2..=2"
    );
    assert_eq!(
        validate_sampling_args(None, None, None, None, Some(-2.1)).unwrap_err(),
        "presencePenalty should be within -2..=2"
    );
}

#[test]
fn preview_headers_map_masks_sensitive_custom_headers() {
    let headers = preview_headers_map(
        Some("sk-secret-123456"),
        Some(ProviderAuth::Bearer),
        &[
            (
                "Authorization".to_string(),
                "Bearer internal-secret-abcdef".to_string(),
            ),
            ("x-api-key".to_string(), "gemini-secret-xyz".to_string()),
            ("X-Trace".to_string(), "trace-123".to_string()),
        ],
    );

    assert_eq!(headers["Authorization"], "Bearer intern***cdef");
    assert_eq!(headers["x-api-key"], "gemini***-xyz");
    assert_eq!(headers["X-Trace"], "trace-123");
}

#[test]
fn provider_catalog_includes_expected_provider_ids() {
    let catalog = provider_catalog();

    assert!(catalog.iter().any(|entry| entry["id"] == "openai"));
    assert!(catalog.iter().any(|entry| entry["id"] == "google-gemini"));
    assert!(catalog.iter().any(|entry| entry["id"] == "qwen"));
}

#[test]
fn openai_compatible_request_payload_omits_top_k_for_openai_base_url() {
    let request = WebLlmChatRequest {
        message: "hello".to_string(),
        ..Default::default()
    };
    let runtime = WebLlmRuntimeConfig {
        base_url: "https://api.openai.com/v1".to_string(),
        model: "gpt-5".to_string(),
        timeout_ms: 30_000,
        system_prompt: Some("system prompt".to_string()),
        stream: false,
        temperature: Some(0.3),
        top_p: Some(0.9),
        top_k: Some(42),
        frequency_penalty: None,
        presence_penalty: None,
        parallel_tool_calls: false,
        reasoning_effort: Some("medium".to_string()),
        default_headers: HashMap::new(),
    };

    let payload = build_openai_compatible_request_payload(&request, &runtime, "persona", "openai")
        .expect("payload should build");

    assert_eq!(payload["model"], "gpt-5");
    assert!(payload.get("top_k").is_none());
    assert_eq!(payload["reasoning"]["effort"], "medium");
    assert_eq!(payload["instructions"], "system prompt");
}

#[test]
fn chat_completions_request_payload_keeps_top_k_and_reasoning() {
    let request = WebLlmChatRequest {
        message: "hello".to_string(),
        ..Default::default()
    };
    let runtime = WebLlmRuntimeConfig {
        base_url: "https://example.com/v1".to_string(),
        model: "qwen-max".to_string(),
        timeout_ms: 30_000,
        system_prompt: None,
        stream: false,
        temperature: Some(0.2),
        top_p: Some(0.8),
        top_k: Some(12),
        frequency_penalty: None,
        presence_penalty: None,
        parallel_tool_calls: false,
        reasoning_effort: Some("high".to_string()),
        default_headers: HashMap::new(),
    };

    let payload =
        build_chat_completions_request_payload(&request, Some("system"), "persona", &runtime)
            .expect("payload should build");

    assert_eq!(payload["model"], "qwen-max");
    assert_eq!(payload["top_k"], 12);
    assert_eq!(payload["reasoning"]["effort"], "high");
    assert_eq!(payload["messages"][0]["role"], "system");
}

#[test]
fn normalize_reasoning_effort_maps_none_to_absent() {
    assert_eq!(normalize_reasoning_effort_input("none"), None);
    assert_eq!(normalize_reasoning_effort_input(" None "), None);
    assert_eq!(normalize_reasoning_effort_input("medium"), Some("medium"));
}

#[test]
fn web_llm_chat_request_deserializes_camel_case_fields() {
    let payload = json!({
        "message": "hello",
        "baseUrl": " https://api.openai.com/v1 ",
        "model": "gpt-5-mini",
        "reasoningEffort": "medium",
        "temperature": 0.3,
        "topP": 0.8,
        "topK": 40,
        "frequencyPenalty": 0.2,
        "presencePenalty": -0.1
    });

    let request: WebLlmChatRequest =
        serde_json::from_value(payload).expect("chat request should deserialize");

    assert_eq!(request.message, "hello");
    assert_eq!(
        request.base_url.as_deref(),
        Some(" https://api.openai.com/v1 ")
    );
    assert_eq!(request.reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(request.temperature, Some(0.3));
    assert_eq!(request.top_p, Some(0.8));
    assert_eq!(request.top_k, Some(40));
    assert_eq!(request.frequency_penalty, Some(0.2));
    assert_eq!(request.presence_penalty, Some(-0.1));
}

#[test]
fn manager_payloads_deserialize_camel_case_fields() {
    let payload = json!({
        "activeProviderId": "provider-1",
        "providers": [{
            "id": "provider-1",
            "label": "OpenAI",
            "providerId": "openai",
            "baseUrl": "https://api.openai.com/v1",
            "apiKey": "sk-test",
            "timeoutSeconds": 90,
            "headers": { "X-Test": "ok" },
            "models": [{ "id": "gpt-5", "enabled": true }]
        }]
    });

    let request: WebLlmManagerSaveRequest =
        serde_json::from_value(payload).expect("manager request should deserialize");

    assert_eq!(request.active_provider_id.as_deref(), Some("provider-1"));
    assert_eq!(request.providers.len(), 1);
    assert_eq!(request.providers[0].provider_id.as_deref(), Some("openai"));
    assert_eq!(request.providers[0].timeout_seconds, Some(90));
    assert_eq!(request.providers[0].models[0].id, "gpt-5");
    assert!(request.providers[0].models[0].enabled);
}

#[test]
fn resolve_active_provider_id_falls_back_when_explicit_id_is_missing() {
    let providers = vec![
        LlmManagedProviderConfig {
            id: Some("provider-a".to_string()),
            base_url: Some("https://a.example/v1".to_string()),
            ..Default::default()
        },
        LlmManagedProviderConfig {
            id: Some("provider-b".to_string()),
            base_url: Some("https://b.example/v1".to_string()),
            ..Default::default()
        },
    ];

    let resolved = resolve_active_provider_id(
        Some("missing-provider".to_string()),
        &providers,
        "https://b.example/v1",
    );

    assert_eq!(resolved.as_deref(), Some("provider-b"));
}

#[test]
fn merge_saved_and_discovered_models_deduplicates_and_preserves_saved_enabled_flags() {
    let provider = LlmManagedProviderConfig {
        models: Some(vec![
            LlmManagedModelConfig {
                id: Some("gpt-5".to_string()),
                enabled: Some(false),
            },
            LlmManagedModelConfig {
                id: Some("o3".to_string()),
                enabled: Some(true),
            },
        ]),
        ..Default::default()
    };
    let discovered = vec!["gpt-5".to_string(), "gpt-4.1".to_string()];
    let enabled = std::collections::HashSet::from(["gpt-5".to_string()]);

    let merged = merge_saved_and_discovered_models(&provider, &discovered, &enabled);

    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].id, "gpt-5");
    assert!(merged[0].enabled);
    assert_eq!(merged[1].id, "gpt-4.1");
    assert_eq!(merged[2].id, "o3");
    assert!(merged[2].enabled);
}

#[test]
fn serialize_managed_provider_emits_active_provider_metadata() {
    let provider = LlmManagedProviderConfig {
        id: Some("provider-1".to_string()),
        provider: Some("openai".to_string()),
        base_url: Some("https://api.openai.com/v1".to_string()),
        api_key: Some("sk-test".to_string()),
        timeout_seconds: Some(90),
        headers: Some(HashMap::from([("X-Test".to_string(), "ok".to_string())])),
        models: Some(vec![LlmManagedModelConfig {
            id: Some("gpt-5".to_string()),
            enabled: Some(true),
        }]),
        ..Default::default()
    };

    let serialized = serialize_managed_provider(&provider, Some("provider-1"));

    assert_eq!(serialized["id"], "provider-1");
    assert_eq!(serialized["providerId"], "openai");
    assert_eq!(serialized["providerLabel"], "OpenAI");
    assert_eq!(serialized["baseUrl"], "https://api.openai.com/v1");
    assert_eq!(serialized["headers"]["X-Test"], "ok");
    assert_eq!(serialized["models"][0]["id"], "gpt-5");
    assert_eq!(serialized["models"][0]["enabled"], true);
    assert_eq!(serialized["active"], true);
}

#[test]
fn serialize_managed_provider_models_preserves_merge_results() {
    let provider = LlmManagedProviderConfig {
        models: Some(vec![LlmManagedModelConfig {
            id: Some("o3".to_string()),
            enabled: Some(true),
        }]),
        ..Default::default()
    };
    let discovered = vec!["gpt-5".to_string()];
    let enabled = std::collections::HashSet::from(["gpt-5".to_string()]);

    let merged = merge_saved_and_discovered_models(&provider, &discovered, &enabled);
    let serialized = serialize_managed_provider_models(&merged);

    assert_eq!(serialized.len(), 2);
    assert_eq!(serialized[0]["id"], "gpt-5");
    assert_eq!(serialized[0]["enabled"], true);
    assert_eq!(serialized[1]["id"], "o3");
    assert_eq!(serialized[1]["enabled"], true);
}

#[test]
fn resolve_models_to_test_prefers_explicit_model_over_test_all() {
    let provider = LlmManagedProviderConfig {
        models: Some(vec![
            LlmManagedModelConfig {
                id: Some("gpt-5".to_string()),
                enabled: Some(true),
            },
            LlmManagedModelConfig {
                id: Some("o3".to_string()),
                enabled: Some(false),
            },
        ]),
        ..Default::default()
    };

    let resolved = resolve_models_to_test(&provider, Some(" o1 "), true);

    assert_eq!(resolved, vec!["o1".to_string()]);
}

#[test]
fn frontend_chat_summary_avoids_prompt_content_and_counts_payload() {
    let request = WebLlmChatRequest {
        messages: vec![
            WebLlmMessage {
                role: "system".to_string(),
                content: "hidden system prompt".to_string(),
                attachments: vec![],
            },
            WebLlmMessage {
                role: "user".to_string(),
                content: "hello from frontend".to_string(),
                attachments: vec![WebLlmAttachment {
                    kind: "image".to_string(),
                    name: "clipboard.png".to_string(),
                    media_type: Some("image/png".to_string()),
                    size: Some(12),
                    data_url: Some("data:image/png;base64,abc".to_string()),
                    text: None,
                }],
            },
        ],
        ..Default::default()
    };

    let summary = summarize_frontend_chat_request(
        &request,
        "openai",
        "https://api.openai.com/v1",
        "gpt-5-mini",
        "default",
        Some("medium"),
    );

    assert!(summary.contains("provider=openai"));
    assert!(summary.contains("turns=2"));
    assert!(summary.contains("attachments=1"));
    assert!(summary.contains("input_chars=39"));
    assert!(summary.contains("reasoning=medium"));
    assert!(!summary.contains("hidden system prompt"));
    assert!(!summary.contains("hello from frontend"));
}

#[test]
fn fallback_prompt_keeps_multi_turn_history_in_transcript() {
    let request = WebLlmChatRequest {
        messages: vec![
            WebLlmMessage {
                role: "user".to_string(),
                content: "你好".to_string(),
                attachments: vec![],
            },
            WebLlmMessage {
                role: "assistant".to_string(),
                content: "你好！有什么我可以帮助你的吗？".to_string(),
                attachments: vec![],
            },
            WebLlmMessage {
                role: "user".to_string(),
                content: "0.9的9循环和1是否相等".to_string(),
                attachments: vec![],
            },
        ],
        ..Default::default()
    };

    let prompt =
        compose_chat_fallback_prompt(&request, "只说一句结论").expect("prompt should build");

    assert!(prompt.contains("Prompt profile instruction:\n只说一句结论"));
    assert!(prompt.contains("user:\n你好"));
    assert!(prompt.contains("assistant:\n你好！有什么我可以帮助你的吗？"));
    assert!(prompt.contains("user:\n0.9的9循环和1是否相等"));
}

#[test]
fn preview_payload_redacts_prompt_like_text_fields() {
    let payload = json!({
        "instructions": "secret system prompt",
        "input": [
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "secret composed prompt" }
                ]
            }
        ],
        "model": "gpt-5"
    });

    let redacted = redact_preview_payload(&payload);

    assert_eq!(redacted["instructions"], "<redacted>");
    assert_eq!(redacted["input"][0]["content"][0]["text"], "<redacted>");
    assert_eq!(redacted["model"], "gpt-5");
}

#[test]
fn preview_payload_redacts_chat_completion_content_strings() {
    let payload = json!({
        "model": "openai/gpt-5",
        "messages": [
            {
                "role": "system",
                "content": "secret system prompt"
            },
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "secret user prompt" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,abc" } }
                ]
            }
        ]
    });

    let redacted = redact_preview_payload(&payload);

    assert_eq!(redacted["messages"][0]["content"], "<redacted>");
    assert_eq!(redacted["messages"][1]["content"][0]["text"], "<redacted>");
    assert_eq!(
        redacted["messages"][1]["content"][1]["image_url"]["url"],
        "data:image/png;base64,abc"
    );
}

#[test]
fn preview_payload_redacts_anthropic_system_prompt() {
    let payload = json!({
        "model": "claude-sonnet-4-20250514",
        "system": "secret system prompt",
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": "hello" }
            ]
        }]
    });

    let redacted = redact_preview_payload(&payload);

    assert_eq!(redacted["system"], "<redacted>");
    assert_eq!(redacted["messages"][0]["content"][0]["text"], "<redacted>");
}

#[test]
fn runtime_supports_follow_provider_catalog_capabilities() {
    let anthropic = current_provider_supports("anthropic", "https://api.anthropic.com", &[]);
    let kimi = current_provider_supports("kimi", "https://api.moonshot.ai/v1", &[]);
    let qwen = current_provider_supports(
        "qwen",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        &[],
    );
    let openai = current_provider_supports(
        "openai",
        "https://api.openai.com/v1",
        &reasoning_options_for_provider("openai", "gpt-5"),
    );

    assert_eq!(anthropic["imageInput"], true);
    assert_eq!(anthropic["textFileInput"], false);
    assert_eq!(kimi["imageInput"], true);
    assert_eq!(kimi["reasoningEffort"], false);
    assert_eq!(qwen["topK"], false);
    assert_eq!(openai["topK"], false);
    assert_eq!(openai["reasoningEffort"], true);
}

#[test]
fn detect_provider_id_recognizes_qwen_international_hosts() {
    assert_eq!(
        detect_provider_id("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        "qwen"
    );
}

#[test]
fn normalize_single_provider_input_keeps_explicit_provider_and_trims_headers() {
    let provider = WebLlmManagedProviderPayload {
        id: " Gemini Main ".to_string(),
        label: " Gemini Main ".to_string(),
        provider_id: Some("google-gemini".to_string()),
        base_url: " https://generativelanguage.googleapis.com/v1beta ".to_string(),
        api_key: " secret ".to_string(),
        timeout_seconds: Some(0),
        headers: HashMap::from([
            (" X-Test ".to_string(), " ok ".to_string()),
            (" ".to_string(), "ignored".to_string()),
        ]),
        models: vec![WebLlmManagedModelPayload {
            id: " gemini-2.5-pro ".to_string(),
            enabled: true,
        }],
    };

    let normalized = normalize_single_provider_input(&provider, 0, 120);

    assert_eq!(normalized.id.as_deref(), Some("gemini-main"));
    assert_eq!(normalized.label.as_deref(), Some("Gemini Main"));
    assert_eq!(normalized.provider.as_deref(), Some("google-gemini"));
    assert_eq!(
        normalized.base_url.as_deref(),
        Some("https://generativelanguage.googleapis.com/v1beta")
    );
    assert_eq!(normalized.api_key.as_deref(), Some("secret"));
    assert_eq!(normalized.timeout_seconds, Some(1));
    assert_eq!(
        normalized
            .headers
            .as_ref()
            .and_then(|headers| headers.get("X-Test"))
            .map(String::as_str),
        Some("ok")
    );
    assert_eq!(
        normalized
            .models
            .as_ref()
            .and_then(|models| models.first())
            .and_then(|model| model.id.as_deref()),
        Some("gemini-2.5-pro")
    );
}

#[test]
fn discover_models_for_provider_reports_catalog_when_remote_lookup_falls_back() {
    let provider = LlmManagedProviderConfig {
        provider: Some("openai".to_string()),
        base_url: Some(String::new()),
        ..Default::default()
    };

    let (models, source) =
        discover_models_for_provider(&provider).expect("fallback discovery should succeed");

    assert_eq!(source, "catalog");
    assert!(models.iter().any(|model| model == "gpt-5"));
}

#[test]
fn reject_non_post_method_returns_method_error_response() {
    let response = reject_non_post_method("GET", "LLM/SaveManagerState", false).expect("response");
    let text = String::from_utf8(response).expect("response should be utf8");
    assert!(text.contains("LLM/SaveManagerState only accepts POST"));
    assert!(reject_non_post_method("POST", "LLM/SaveManagerState", false).is_none());
}

#[test]
fn route_llm_api_rejects_non_post_manager_routes() {
    let service = test_web_host_service();

    for route_name in [
        "/LLM/SaveManagerState",
        "/LLM/UpdateEnabled",
        "/LLM/FetchModels",
        "/LLM/TestModels",
        "/LLM/PreviewRequest",
        "/LLM/PromptProfiles/Save",
        "/LLM/PromptProfiles/Delete",
        "/LLM/PromptProfiles/Use",
        "/LLM/PromptProfiles/Preview",
    ] {
        let response = route_llm_api(&service, "GET", route_name, b"{}", false).expect("response");
        let text = String::from_utf8(response).expect("response should be utf8");
        assert!(
            text.contains("only accepts POST"),
            "route {route_name} should reject GET"
        );
    }
}

#[test]
fn route_llm_api_returns_none_for_unknown_path() {
    let service = test_web_host_service();
    assert!(route_llm_api(&service, "GET", "/LLM/Unknown", b"{}", false).is_none());
}

#[test]
fn resolve_llm_enabled_write_path_uses_active_app_config_without_overlay() {
    let _env_guard = process_state_lock();
    let root =
        std::env::temp_dir().join(format!("rsliteyukibot-llm-enabled-{}", std::process::id()));
    let config_dir = root.join(".liteyuki").join("configs");
    fs::create_dir_all(&config_dir).expect("config dir should be created");

    let userprofile_guard = EnvVarGuard::set("USERPROFILE", &root);
    let home_guard = EnvVarGuard::remove("HOME");
    let llm_guard = EnvVarGuard::remove("LY_LLM_CONFIG_PATH");
    let cwd_guard = CurrentDirGuard::set(&root);

    let path =
        resolve_llm_enabled_write_path_for_web().expect("app config fallback should resolve");

    assert_eq!(path, config_dir.join("config.yaml"));
    assert!(!config_dir.join("llm-config.yaml").exists());

    drop(llm_guard);
    drop(home_guard);
    drop(userprofile_guard);
    drop(cwd_guard);
    let _ = fs::remove_dir_all(&root);
}
fn test_web_host_service() -> WebHostService {
    let assets = WebHostAssets::new(WebHostAsset::text(
        "text/html; charset=utf-8",
        "<html></html>",
    ));
    let snapshot_provider = std::sync::Arc::new(AppHostSnapshot::default);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    let _guard = runtime.enter();
    let (service, _listener) = WebHostService::bind(
        WebHostConfig {
            port: 0,
            ..WebHostConfig::default()
        },
        snapshot_provider,
        assets,
    )
    .expect("test web host service should bind");
    service
}
