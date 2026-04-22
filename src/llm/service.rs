use std::path::PathBuf;

use crate::app_config::{AppConfigDoc, LlmRuntimeConfig, resolve_llm_config};
use crate::i18n::{tr, trf};
use crate::llm::{
    LlmClientError, LlmPromptProfile, LlmPromptStore, OpenAiResponsesClient, compose_user_prompt,
};
use crate::runtime_support::{
    LLM_PROMPT_STORE_PATH, load_app_config_with_llm_overlay, next_llm_api_key_index,
};

pub(crate) async fn generate_llm_reply(prompt: &str) -> Result<String, String> {
    let llm_config = current_llm_runtime_config()?;
    if !llm_config.enabled {
        return Err(tr("main.llm.disabled"));
    }
    if !llm_config.provider.eq_ignore_ascii_case("openai") {
        return Err(trf(
            "main.llm.provider.unsupported",
            &[("provider", llm_config.provider.as_str())],
        ));
    }
    let Some(api_key) = pick_next_api_key(&llm_config) else {
        return Err(tr("main.llm.api_key_missing"));
    };
    let prompt_profile = current_active_prompt_profile()?;
    let composed_prompt = compose_user_prompt(prompt, prompt_profile.soul.as_str());

    let client = OpenAiResponsesClient::from_runtime_with_api_key(&llm_config, &api_key)
        .map_err(|err: LlmClientError| err.to_string())?;
    client
        .generate(composed_prompt.as_str())
        .await
        .map_err(|err| err.to_string())
}

pub(crate) fn current_llm_runtime_config() -> Result<LlmRuntimeConfig, String> {
    let doc = load_current_app_config_doc()?;
    Ok(resolve_llm_config(&doc))
}

pub(crate) fn load_current_app_config_doc() -> Result<AppConfigDoc, String> {
    let (doc, _) = load_app_config_with_llm_overlay();
    Ok(doc)
}

pub(crate) fn current_active_prompt_profile() -> Result<LlmPromptProfile, String> {
    let store = load_llm_prompt_store()?;
    Ok(store.active_profile())
}

pub(crate) fn load_llm_prompt_store() -> Result<LlmPromptStore, String> {
    let path = resolve_llm_prompt_store_path();
    LlmPromptStore::load_or_default_from_path(path.as_path())
}

#[allow(dead_code)]
pub(crate) fn persist_llm_prompt_store(store: &LlmPromptStore) -> Result<PathBuf, String> {
    let path = resolve_llm_prompt_store_path();
    store.save_to_path(path.as_path())?;
    Ok(path)
}

pub(crate) fn resolve_llm_prompt_store_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_LLM_PROMPT_STORE_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    PathBuf::from(LLM_PROMPT_STORE_PATH)
}

fn pick_next_api_key(llm_config: &LlmRuntimeConfig) -> Option<String> {
    let index = next_llm_api_key_index(llm_config.api_keys.len())?;
    llm_config.api_keys.get(index).cloned()
}
