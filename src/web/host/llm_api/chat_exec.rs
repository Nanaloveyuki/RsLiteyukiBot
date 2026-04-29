use super::{
    ProviderAuth, WebLlmChatExecution, WebLlmChatRequest, WebLlmRuntimeConfig,
    build_anthropic_request, build_chat_completions_messages, build_gemini_request,
    build_responses_input, collect_runtime_headers, extract_anthropic_text, extract_gemini_text,
    join_api_endpoint, send_json_request,
};

use crate::LlmFunctionTool;
use crate::app_config::LlmRuntimeConfig;
use crate::llm::tools::{ToolManager, merge_system_prompt_sections};
use crate::llm::{LlmClientError, OpenAiResponsesClient};
use crate::web::host::{WebHostService, run_async_for_web_host};

const DEFAULT_ANTHROPIC_API_VERSION: &str = "2023-06-01";

pub(super) async fn execute_provider_chat(
    service: &WebHostService,
    request: &WebLlmChatRequest,
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    provider_id: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    match crate::llm::service::provider_api_family(provider_id) {
        crate::llm::service::LlmProviderApiFamily::AnthropicMessages => {
            send_anthropic_chat(
                request,
                llm_config,
                api_key,
                soul,
                effective_base_url,
                effective_model,
                reasoning_effort,
            )
            .await
        }
        crate::llm::service::LlmProviderApiFamily::GeminiGenerateContent => {
            send_gemini_chat(
                request,
                llm_config,
                api_key,
                soul,
                effective_base_url,
                effective_model,
                reasoning_effort,
            )
            .await
        }
        crate::llm::service::LlmProviderApiFamily::ChatCompletions => {
            send_chat_completions_family_chat(
                service,
                request,
                llm_config,
                api_key,
                soul,
                effective_base_url,
                effective_model,
                reasoning_effort,
            )
            .await
        }
        crate::llm::service::LlmProviderApiFamily::Responses => {
            send_openai_compatible_chat(
                service,
                request,
                llm_config,
                api_key,
                soul,
                provider_id,
                effective_base_url,
                effective_model,
                reasoning_effort,
            )
            .await
        }
    }
}

async fn send_openai_compatible_chat(
    service: &WebHostService,
    request: &WebLlmChatRequest,
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    provider_id: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    let fallback_prompt = super::compose_chat_fallback_prompt(request, soul)?;
    let responses_input = build_responses_input(request, soul, provider_id)?;
    let plugin_tools = runtime_plugin_tools(service)?;
    let capability_bundle = ToolManager::for_current_workspace()?
        .build_runtime_bundle(plugin_tools.as_slice())
        .await?;
    let effective_config = build_web_runtime_config(
        request,
        llm_config,
        effective_base_url,
        effective_model,
        merge_system_prompt_sections(
            llm_config.system_prompt.as_deref(),
            [capability_bundle.system_prompt.as_deref()],
        ),
        reasoning_effort,
    );

    let client = OpenAiResponsesClient::from_runtime_with_api_key(&effective_config, api_key)
        .map_err(|err: LlmClientError| err.to_string())?;
    let completion = client
        .complete_with_input(
            fallback_prompt.as_str(),
            responses_input,
            capability_bundle.tools.as_slice(),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;

    Ok(WebLlmChatExecution {
        message: completion.text,
        model: effective_model.to_string(),
        base_url: effective_base_url.to_string(),
    })
}

async fn send_chat_completions_family_chat(
    service: &WebHostService,
    request: &WebLlmChatRequest,
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    let plugin_tools = runtime_plugin_tools(service)?;
    let capability_bundle = ToolManager::for_current_workspace()?
        .build_runtime_bundle(plugin_tools.as_slice())
        .await?;
    let merged_system_prompt = merge_system_prompt_sections(
        llm_config.system_prompt.as_deref(),
        [capability_bundle.system_prompt.as_deref()],
    );
    let messages = build_chat_completions_messages(request, merged_system_prompt.as_deref(), soul)?;
    let effective_config = build_web_runtime_config(
        request,
        llm_config,
        effective_base_url,
        effective_model,
        None,
        reasoning_effort,
    );
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&effective_config, api_key)
        .map_err(|err: LlmClientError| err.to_string())?;
    let completion = client
        .complete_with_chat_messages(messages, capability_bundle.tools.as_slice(), None)
        .await
        .map_err(|err| err.to_string())?;

    Ok(WebLlmChatExecution {
        message: completion.text,
        model: effective_model.to_string(),
        base_url: effective_base_url.to_string(),
    })
}

fn runtime_plugin_tools(service: &WebHostService) -> Result<Vec<LlmFunctionTool>, String> {
    match service.runtime_host.as_ref() {
        Some(runtime_host) => run_async_for_web_host(runtime_host.build_all_plugin_tool_bundle()),
        None => Ok(Vec::new()),
    }
}

async fn send_anthropic_chat(
    request: &WebLlmChatRequest,
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    let endpoint = join_api_endpoint(effective_base_url, "v1/messages");
    let payload = build_anthropic_request(
        request,
        llm_config.system_prompt.as_deref(),
        soul,
        effective_model,
        request.temperature.or(llm_config.temperature),
        request.top_p.or(llm_config.top_p),
        request.top_k.or(llm_config.top_k),
        reasoning_effort,
    )?;
    let response = send_json_request(
        llm_config.timeout_ms,
        endpoint.as_str(),
        api_key,
        ProviderAuth::ApiKeyHeader("x-api-key".to_string()),
        &collect_runtime_headers(
            &llm_config.headers,
            &[("anthropic-version", DEFAULT_ANTHROPIC_API_VERSION)],
        ),
        &payload,
    )
    .await?;
    let message = extract_anthropic_text(&response)
        .ok_or_else(|| "Anthropic response did not contain assistant text".to_string())?;

    Ok(WebLlmChatExecution {
        message,
        model: response
            .pointer("/model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(effective_model)
            .to_string(),
        base_url: effective_base_url.to_string(),
    })
}

async fn send_gemini_chat(
    request: &WebLlmChatRequest,
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
    soul: &str,
    effective_base_url: &str,
    effective_model: &str,
    reasoning_effort: Option<&str>,
) -> Result<WebLlmChatExecution, String> {
    let endpoint = join_api_endpoint(
        effective_base_url,
        format!("models/{effective_model}:generateContent").as_str(),
    );
    let payload = build_gemini_request(
        request,
        llm_config.system_prompt.as_deref(),
        soul,
        effective_model,
        request.temperature.or(llm_config.temperature),
        request.top_p.or(llm_config.top_p),
        request.top_k.or(llm_config.top_k),
        reasoning_effort,
    )?;
    let response = send_json_request(
        llm_config.timeout_ms,
        endpoint.as_str(),
        api_key,
        ProviderAuth::ApiKeyHeader("x-goog-api-key".to_string()),
        &collect_runtime_headers(&llm_config.headers, &[]),
        &payload,
    )
    .await?;
    let message = extract_gemini_text(&response)
        .ok_or_else(|| "Gemini response did not contain assistant text".to_string())?;

    Ok(WebLlmChatExecution {
        message,
        model: effective_model.to_string(),
        base_url: effective_base_url.to_string(),
    })
}

fn build_web_runtime_config(
    request: &WebLlmChatRequest,
    llm_config: &LlmRuntimeConfig,
    effective_base_url: &str,
    effective_model: &str,
    system_prompt: Option<String>,
    reasoning_effort: Option<&str>,
) -> WebLlmRuntimeConfig {
    WebLlmRuntimeConfig {
        base_url: effective_base_url.to_string(),
        model: effective_model.to_string(),
        timeout_ms: llm_config.timeout_ms,
        system_prompt,
        stream: false,
        temperature: request.temperature.or(llm_config.temperature),
        top_p: request.top_p.or(llm_config.top_p),
        top_k: request.top_k.or(llm_config.top_k),
        frequency_penalty: request.frequency_penalty.or(llm_config.frequency_penalty),
        presence_penalty: request.presence_penalty.or(llm_config.presence_penalty),
        parallel_tool_calls: llm_config.parallel_tool_calls,
        reasoning_effort: reasoning_effort.map(ToString::to_string),
        default_headers: llm_config.headers.clone(),
    }
}
