use serde_json::{Value, json};

use super::{
    WebLlmPromptProfileNameRequest, WebLlmPromptProfilePreviewRequest,
    WebLlmPromptProfileSaveRequest, parse_json_body,
};

use crate::llm::service::{
    current_llm_runtime_config, load_llm_prompt_store, persist_llm_prompt_store,
    resolve_llm_prompt_store_path,
};
use crate::llm::{LlmPromptPreview, LlmPromptProfile, LlmPromptStore, build_prompt_preview};

pub(super) fn llm_prompt_profiles_payload() -> Result<Value, String> {
    let store = load_llm_prompt_store()?;
    Ok(serialize_prompt_store_payload(&store))
}

pub(super) fn save_llm_prompt_profile(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmPromptProfileSaveRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid prompt profile save payload: {err}"))?;
    let mut store = load_llm_prompt_store()?;
    store.upsert_profile(payload.name.as_str(), payload.soul.as_str())?;
    if payload.active.unwrap_or(false) {
        store.set_active_profile(payload.name.as_str())?;
    }
    persist_llm_prompt_store(&store)?;
    Ok(serialize_prompt_store_payload(&store))
}

pub(super) fn delete_llm_prompt_profile(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmPromptProfileNameRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid prompt profile delete payload: {err}"))?;
    let mut store = load_llm_prompt_store()?;
    store.remove_profile(payload.name.as_str())?;
    persist_llm_prompt_store(&store)?;
    Ok(serialize_prompt_store_payload(&store))
}

pub(super) fn use_llm_prompt_profile(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmPromptProfileNameRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid prompt profile use payload: {err}"))?;
    let mut store = load_llm_prompt_store()?;
    store.set_active_profile(payload.name.as_str())?;
    persist_llm_prompt_store(&store)?;
    Ok(serialize_prompt_store_payload(&store))
}

pub(super) fn preview_llm_prompt_profile(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmPromptProfilePreviewRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid prompt profile preview payload: {err}"))?;
    let store = load_llm_prompt_store()?;
    let profile = resolve_prompt_profile_for_preview(&store, payload.name.as_deref())?;
    let llm_config = current_llm_runtime_config()?;
    let preview = build_prompt_preview(
        payload
            .system_prompt
            .as_deref()
            .or(llm_config.system_prompt.as_deref()),
        payload.user_prompt.unwrap_or_default().as_str(),
        profile.soul.as_str(),
    );

    Ok(serialize_prompt_preview_payload(&store, &profile, &preview))
}

pub(super) fn serialize_prompt_store_payload(store: &LlmPromptStore) -> Value {
    let store = store.normalized();
    let mut profiles = store
        .profiles
        .iter()
        .map(|profile| serialize_prompt_profile(profile, store.active_profile.as_str()))
        .collect::<Vec<_>>();
    profiles.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["name"].as_str().unwrap_or_default())
    });

    json!({
        "configPath": resolve_llm_prompt_store_path().display().to_string(),
        "activeProfile": store.active_profile,
        "profiles": profiles,
    })
}

fn serialize_prompt_preview_payload(
    store: &LlmPromptStore,
    profile: &LlmPromptProfile,
    preview: &LlmPromptPreview,
) -> Value {
    json!({
        "configPath": resolve_llm_prompt_store_path().display().to_string(),
        "activeProfile": store.normalized().active_profile,
        "profile": serialize_prompt_profile(profile, store.active_profile.as_str()),
        "preview": {
            "systemPrompt": preview.system_prompt,
            "composedUserPrompt": preview.composed_user_prompt,
            "combinedPrompt": preview.combined_prompt,
        }
    })
}

fn serialize_prompt_profile(profile: &LlmPromptProfile, active_profile: &str) -> Value {
    json!({
        "name": profile.name,
        "soul": profile.soul,
        "active": profile.name == active_profile,
        "canDelete": profile.name != "default",
    })
}

fn resolve_prompt_profile_for_preview(
    store: &LlmPromptStore,
    requested_name: Option<&str>,
) -> Result<LlmPromptProfile, String> {
    let normalized_name = requested_name
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(name) = normalized_name {
        return store
            .normalized()
            .profiles
            .into_iter()
            .find(|profile| profile.name == name)
            .ok_or_else(|| format!("prompt profile not found: {name}"));
    }
    Ok(store.active_profile())
}
