#[path = "service/config_store.rs"]
mod config_store;
#[path = "service/provider_runtime.rs"]
mod provider_runtime;
#[path = "service/provider_selection.rs"]
mod provider_selection;

use crate::app_config::LlmRuntimeConfig;
use crate::i18n::tr;
use crate::llm::tools::{ToolManager, merge_system_prompt_sections};
use crate::llm::{
    LlmClientError, LlmCompletion, LlmEventSink, LlmFunctionTool, OpenAiResponsesClient,
    compose_user_prompt,
};

use self::config_store::pick_next_api_key;
use self::provider_runtime::complete_prompt_without_tool_runtime;

pub(crate) use self::provider_selection::LlmProviderApiFamily;

pub(crate) async fn generate_llm_reply(prompt: &str) -> Result<String, String> {
    complete_llm_prompt(prompt, &[], None)
        .await
        .map(|completion| completion.text)
}

// 外部调用
#[allow(dead_code)]
pub(crate) async fn generate_llm_reply_with_events(
    prompt: &str,
    sink: &mut dyn LlmEventSink,
) -> Result<LlmCompletion, String> {
    complete_llm_prompt(prompt, &[], Some(sink)).await
}

// 外部调用
#[allow(dead_code)]
pub(crate) async fn complete_llm_prompt(
    prompt: &str,
    tools: &[LlmFunctionTool],
    sink: Option<&mut dyn LlmEventSink>,
) -> Result<LlmCompletion, String> {
    let llm_config = current_llm_runtime_config()?;
    if !llm_config.enabled {
        return Err(tr("main.llm.disabled"));
    }
    let Some(api_key) = pick_next_api_key(&llm_config) else {
        return Err(tr("main.llm.api_key_missing"));
    };
    let prompt_profile = current_active_prompt_profile()?;
    let composed_prompt = compose_user_prompt(prompt, prompt_profile.soul.as_str());
    let provider_id = resolve_runtime_provider_id(&llm_config);
    let provider_family = provider_api_family(provider_id.as_str());

    match provider_family {
        LlmProviderApiFamily::Responses | LlmProviderApiFamily::ChatCompletions => {
            let mut llm_config = llm_config;
            let capability_bundle = ToolManager::for_current_workspace()?
                .build_runtime_bundle(tools)
                .await?;
            llm_config.system_prompt = merge_system_prompt_sections(
                llm_config.system_prompt.as_deref(),
                [capability_bundle.system_prompt.as_deref()],
            );

            let client = OpenAiResponsesClient::from_runtime_with_api_key(&llm_config, &api_key)
                .map_err(|err: LlmClientError| err.to_string())?;
            match provider_family {
                LlmProviderApiFamily::Responses => client
                    .complete(
                        composed_prompt.as_str(),
                        capability_bundle.tools.as_slice(),
                        sink,
                    )
                    .await
                    .map_err(|err| err.to_string()),
                LlmProviderApiFamily::ChatCompletions => client
                    .complete_with_chat_completions(
                        composed_prompt.as_str(),
                        capability_bundle.tools.as_slice(),
                        sink,
                    )
                    .await
                    .map_err(|err| err.to_string()),
                _ => unreachable!("provider family already matched above"),
            }
        }
        LlmProviderApiFamily::AnthropicMessages | LlmProviderApiFamily::GeminiGenerateContent => {
            let text = complete_prompt_without_tool_runtime(
                composed_prompt.as_str(),
                &llm_config,
                &api_key,
                provider_id.as_str(),
            )
            .await?;
            Ok(LlmCompletion {
                text,
                tool_calls: Vec::new(),
            })
        }
    }
}

// 外部调用
#[allow(dead_code)]
pub(crate) async fn probe_llm_runtime_text(
    llm_config: &LlmRuntimeConfig,
    api_key: &str,
    prompt: &str,
) -> Result<String, String> {
    let provider_id = resolve_runtime_provider_id(llm_config);
    match provider_api_family(provider_id.as_str()) {
        LlmProviderApiFamily::Responses => {
            let client = OpenAiResponsesClient::from_runtime_with_api_key(llm_config, api_key)
                .map_err(|err| err.to_string())?;
            client.generate(prompt).await.map_err(|err| err.to_string())
        }
        LlmProviderApiFamily::ChatCompletions => {
            let client = OpenAiResponsesClient::from_runtime_with_api_key(llm_config, api_key)
                .map_err(|err| err.to_string())?;
            client
                .generate_with_chat_completions(prompt)
                .await
                .map_err(|err| err.to_string())
        }
        LlmProviderApiFamily::AnthropicMessages | LlmProviderApiFamily::GeminiGenerateContent => {
            complete_prompt_without_tool_runtime(prompt, llm_config, api_key, provider_id.as_str())
                .await
        }
    }
}

pub(crate) fn current_llm_runtime_config() -> Result<LlmRuntimeConfig, String> {
    config_store::current_llm_runtime_config()
}

pub(crate) fn load_current_app_config_doc() -> Result<crate::app_config::AppConfigDoc, String> {
    config_store::load_current_app_config_doc()
}

pub(crate) fn current_active_prompt_profile() -> Result<crate::llm::LlmPromptProfile, String> {
    config_store::current_active_prompt_profile()
}

pub(crate) fn load_llm_prompt_store() -> Result<crate::llm::LlmPromptStore, String> {
    config_store::load_llm_prompt_store()
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn persist_llm_prompt_store(
    store: &crate::llm::LlmPromptStore,
) -> Result<std::path::PathBuf, String> {
    config_store::persist_llm_prompt_store(store)
}

pub(crate) fn resolve_llm_prompt_store_path() -> std::path::PathBuf {
    config_store::resolve_llm_prompt_store_path()
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn provider_uses_openai_runtime(provider_id: &str) -> bool {
    provider_selection::provider_uses_openai_runtime(provider_id)
}

pub(crate) fn provider_api_family(provider_id: &str) -> LlmProviderApiFamily {
    provider_selection::provider_api_family(provider_id)
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn resolve_provider_id(
    configured_provider: Option<&str>,
    base_url: Option<&str>,
) -> String {
    provider_selection::resolve_provider_id(configured_provider, base_url)
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn detect_provider_id_from_base_url(base_url: &str) -> String {
    provider_selection::detect_provider_id_from_base_url(base_url)
}

pub(crate) fn resolve_runtime_provider_id(llm_config: &LlmRuntimeConfig) -> String {
    provider_selection::resolve_runtime_provider_id(llm_config)
}
