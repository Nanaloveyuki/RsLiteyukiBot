use std::path::PathBuf;

use crate::app_config::{AppConfigDoc, LlmRuntimeConfig, resolve_llm_config};
use crate::llm::{LlmPromptProfile, LlmPromptStore};
use crate::runtime_support::{load_app_config_with_llm_overlay, next_llm_api_key_index};
use crate::utils::config_path::resolve_preferred_llm_prompt_store_path;

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

// 外部调用
#[allow(dead_code)]
pub(crate) fn persist_llm_prompt_store(store: &LlmPromptStore) -> Result<PathBuf, String> {
    let path = resolve_llm_prompt_store_path();
    store.save_to_path(path.as_path())?;
    Ok(path)
}

pub(crate) fn resolve_llm_prompt_store_path() -> PathBuf {
    resolve_preferred_llm_prompt_store_path()
}

pub(super) fn pick_next_api_key(llm_config: &LlmRuntimeConfig) -> Option<String> {
    let index = next_llm_api_key_index(llm_config.api_keys.len())?;
    llm_config.api_keys.get(index).cloned()
}
