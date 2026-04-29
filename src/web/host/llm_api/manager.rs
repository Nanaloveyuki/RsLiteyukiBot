use serde_json::{Value, json};

use super::{
    WebLlmManagerSaveRequest, WebLlmProviderActionRequest, configured_provider_options,
    current_provider_supports, discover_models_for_provider, load_managed_providers_from_doc,
    managed_provider_model_options, merge_saved_and_discovered_models, normalize_base_url,
    normalize_managed_provider_inputs, normalize_single_provider_input, parse_json_body,
    prepare_provider_request, preview_headers_map, probe_single_provider_model, provider_catalog,
    provider_enabled_model_set, provider_label, reasoning_options_for_provider,
    redact_preview_payload, resolve_active_provider_id, resolve_models_to_test,
    select_primary_model_id, serialize_managed_provider, serialize_managed_provider_models,
};

use crate::config_edit::LlmConfigPatch;
use crate::llm::service::{
    current_active_prompt_profile, current_llm_runtime_config, load_current_app_config_doc,
    resolve_provider_id,
};
use crate::runtime_support::{ensure_llm_config_file, resolve_llm_config_path};
use crate::utils::config_path::{resolve_default_app_config_path, resolve_default_llm_config_path};

pub(super) fn llm_settings_payload() -> Result<Value, String> {
    let llm_config = current_llm_runtime_config()?;
    let prompt_profile = current_active_prompt_profile()?;
    let doc = load_current_app_config_doc()?;
    let managed_providers = load_managed_providers_from_doc(&doc, &llm_config);
    let base_url = normalize_base_url(llm_config.base_url.as_str());
    let provider_id =
        resolve_provider_id(Some(llm_config.provider.as_str()), Some(base_url.as_str()));
    let active_provider_id = resolve_active_provider_id(
        doc.llm
            .as_ref()
            .and_then(|section| section.active_provider_id.clone()),
        &managed_providers,
        base_url.as_str(),
    );
    let active_provider = active_provider_id.as_deref().and_then(|active_id| {
        managed_providers
            .iter()
            .find(|provider| provider.id.as_deref() == Some(active_id))
    });
    let provider_options = configured_provider_options(base_url.as_str(), provider_id.as_str())?;
    let model_options = managed_provider_model_options(active_provider, provider_id.as_str());
    let reasoning_options =
        reasoning_options_for_provider(provider_id.as_str(), llm_config.model.as_str());
    let provider_display_label = active_provider
        .and_then(|provider| provider.label.as_deref())
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| provider_label(provider_id.as_str()));
    let supports = current_provider_supports(
        provider_id.as_str(),
        base_url.as_str(),
        reasoning_options.as_slice(),
    );

    Ok(json!({
        "enabled": llm_config.enabled,
        "provider": provider_display_label,
        "baseUrl": base_url,
        "model": llm_config.model,
        "activeProviderId": active_provider_id,
        "providerOptions": provider_options,
        "modelOptions": model_options,
        "reasoningOptions": reasoning_options,
        "promptProfile": prompt_profile.name,
        "supports": supports,
        "providerCatalog": provider_catalog(),
    }))
}

pub(super) fn llm_manager_state_payload() -> Result<Value, String> {
    let doc = load_current_app_config_doc()?;
    let runtime = current_llm_runtime_config()?;
    let providers = load_managed_providers_from_doc(&doc, &runtime);
    let active_provider_id = resolve_active_provider_id(
        doc.llm
            .as_ref()
            .and_then(|section| section.active_provider_id.clone()),
        &providers,
        runtime.base_url.as_str(),
    );
    let config_path = resolve_llm_config_write_path_for_web();

    Ok(json!({
        "activeProviderId": active_provider_id,
        "configPath": config_path.display().to_string(),
        "providerCatalog": provider_catalog(),
        "providers": providers
            .iter()
            .map(|provider| serialize_managed_provider(provider, active_provider_id.as_deref()))
            .collect::<Vec<_>>(),
    }))
}

pub(super) fn save_llm_manager_state(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let save_request: WebLlmManagerSaveRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid LLM manager save payload: {err}"))?;

    let doc = load_current_app_config_doc()?;
    let runtime = current_llm_runtime_config()?;
    let providers = normalize_managed_provider_inputs(
        save_request.providers.as_slice(),
        runtime.timeout_ms.saturating_div(1000).max(1),
    );
    let active_provider_id = resolve_active_provider_id(
        save_request.active_provider_id,
        &providers,
        runtime.base_url.as_str(),
    );
    let active_provider = active_provider_id.as_deref().and_then(|active_id| {
        providers
            .iter()
            .find(|provider| provider.id.as_deref() == Some(active_id))
    });
    let active_model = active_provider.and_then(select_primary_model_id);
    let fallback_model = doc
        .llm
        .as_ref()
        .and_then(|section| section.model.clone())
        .unwrap_or_else(|| runtime.model.clone());
    let provider_urls = providers
        .iter()
        .filter_map(|provider| provider.base_url.clone())
        .collect::<Vec<_>>();
    let active_api_keys = active_provider
        .and_then(|provider| provider.api_key.clone())
        .map(|value| vec![value])
        .unwrap_or_default();
    let active_headers = active_provider
        .and_then(|provider| provider.headers.clone())
        .unwrap_or_default();
    let timeout_seconds = active_provider
        .and_then(|provider| provider.timeout_seconds)
        .or_else(|| {
            doc.llm
                .as_ref()
                .and_then(|section| section.timeout_seconds)
                .filter(|value| *value > 0)
        })
        .or(Some(runtime.timeout_ms.saturating_div(1000).max(1)));

    let patch = LlmConfigPatch {
        enabled: Some(active_provider.is_some() && active_model.is_some()),
        provider: active_provider.and_then(|provider| provider.provider.clone()),
        base_url: active_provider.and_then(|provider| provider.base_url.clone()),
        provider_urls: Some(provider_urls),
        model: Some(active_model.unwrap_or(fallback_model)),
        api_keys: Some(active_api_keys),
        timeout_seconds,
        headers: Some(active_headers),
        active_provider_id,
        providers: Some(providers),
        ..Default::default()
    };

    let path = resolve_llm_config_write_path_for_web();
    ensure_llm_config_file(path.as_path())?;
    crate::config_edit::persist_llm_config(path.as_path(), &patch)?;
    llm_manager_state_payload()
}

pub(super) fn update_llm_enabled_state(request: &[u8]) -> Result<Value, String> {
    let enabled = parse_json_body(request)
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| "enabled flag is required".to_string())?;
    let path = resolve_llm_enabled_write_path_for_web()?;
    crate::config_edit::persist_llm_config(
        path.as_path(),
        &LlmConfigPatch {
            enabled: Some(enabled),
            ..Default::default()
        },
    )?;
    llm_settings_payload()
}

pub(super) fn llm_fetch_models_payload(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmProviderActionRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid LLM provider payload: {err}"))?;
    let runtime = current_llm_runtime_config()?;
    let provider = normalize_single_provider_input(&payload.provider, 0, runtime.timeout_ms / 1000);
    let (models, source) = discover_models_for_provider(&provider)?;
    let enabled_models = provider_enabled_model_set(&provider);
    let merged_models =
        merge_saved_and_discovered_models(&provider, models.as_slice(), &enabled_models);

    Ok(json!({
        "source": source,
        "provider": serialize_managed_provider(&provider, None),
        "models": serialize_managed_provider_models(&merged_models),
    }))
}

pub(super) fn llm_test_models_payload(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmProviderActionRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid LLM provider test payload: {err}"))?;
    let runtime = current_llm_runtime_config()?;
    let provider = normalize_single_provider_input(&payload.provider, 0, runtime.timeout_ms / 1000);
    let model_ids =
        resolve_models_to_test(&provider, payload.model_id.as_deref(), payload.test_all);
    if model_ids.is_empty() {
        return Err("no provider model available for probing".to_string());
    }

    let results = model_ids
        .into_iter()
        .map(|model_id| probe_single_provider_model(&provider, model_id.as_str()))
        .collect::<Vec<_>>();

    Ok(json!({
        "provider": serialize_managed_provider(&provider, None),
        "results": results,
    }))
}

pub(super) fn llm_preview_request_payload(request: &[u8]) -> Result<Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebLlmProviderActionRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid LLM preview payload: {err}"))?;
    let runtime = current_llm_runtime_config()?;
    let provider = normalize_single_provider_input(&payload.provider, 0, runtime.timeout_ms / 1000);
    let model_id = payload
        .model_id
        .or_else(|| select_primary_model_id(&provider))
        .ok_or_else(|| "no model available for request preview".to_string())?;
    let prepared = prepare_provider_request(&provider, model_id.as_str())?;
    let preview_body = redact_preview_payload(&prepared.payload);

    Ok(json!({
        "provider": serialize_managed_provider(&provider, None),
        "modelId": model_id,
        "method": "POST",
        "endpoint": prepared.endpoint,
        "headers": preview_headers_map(
            prepared.api_key.as_deref(),
            prepared.auth.clone(),
            prepared.extra_headers.as_slice(),
        ),
        "body": preview_body,
        "bodyText": serde_json::to_string_pretty(&preview_body)
            .unwrap_or_else(|err| format!("<failed to render request body: {err}>")),
    }))
}

pub(super) fn resolve_llm_config_write_path_for_web() -> std::path::PathBuf {
    resolve_llm_config_path().unwrap_or_else(resolve_default_llm_config_path)
}

pub(super) fn resolve_llm_enabled_write_path_for_web() -> Result<std::path::PathBuf, String> {
    if let Some(path) = resolve_llm_config_path() {
        return Ok(path);
    }

    crate::app_config::ensure_default_config_files().map_err(|err| err.to_string())?;
    Ok(
        crate::app_config::resolve_app_config_path()
            .unwrap_or_else(resolve_default_app_config_path),
    )
}
