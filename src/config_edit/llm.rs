use std::collections::HashMap;

use crate::app_config::{LlmManagedModelConfig, LlmManagedProviderConfig};
use crate::utils::llm_config::{
    normalize_headers_map, normalize_lowercase_non_empty_string, normalize_non_empty_string,
    normalize_provider_url, normalize_provider_url_entries,
    normalize_string_entries_preserve_order,
};

#[derive(Debug, Clone, Default)]
pub struct LlmConfigPatch {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub provider_urls: Option<Vec<String>>,
    pub model: Option<String>,
    pub api_keys: Option<Vec<String>>,
    pub timeout_seconds: Option<u64>,
    pub headers: Option<HashMap<String, String>>,
    pub active_provider_id: Option<String>,
    pub providers: Option<Vec<LlmManagedProviderConfig>>,
}

pub(crate) fn describe_llm_patch(patch: &LlmConfigPatch) -> String {
    let mut fields = Vec::new();
    if let Some(enabled) = patch.enabled {
        fields.push(format!("enabled={enabled}"));
    }
    if let Some(provider) = patch.provider.as_deref() {
        fields.push(format!("provider={provider}"));
    }
    if let Some(base_url) = patch.base_url.as_deref() {
        fields.push(format!("base_url={base_url}"));
    }
    if let Some(provider_urls) = patch.provider_urls.as_ref() {
        fields.push(format!("provider_urls={}", provider_urls.len()));
    }
    if let Some(model) = patch.model.as_deref() {
        fields.push(format!("model={model}"));
    }
    if let Some(api_keys) = patch.api_keys.as_ref() {
        fields.push(format!("api_keys=<updated:{}>", api_keys.len()));
    }
    if let Some(timeout_seconds) = patch.timeout_seconds {
        fields.push(format!("timeout_seconds={timeout_seconds}"));
    }
    if let Some(headers) = patch.headers.as_ref() {
        fields.push(format!("headers={}", headers.len()));
    }
    if let Some(active_provider_id) = patch.active_provider_id.as_deref() {
        fields.push(format!("active_provider_id={active_provider_id}"));
    }
    if let Some(providers) = patch.providers.as_ref() {
        fields.push(format!("providers={}", providers.len()));
    }

    if fields.is_empty() {
        "fields=none".to_string()
    } else {
        fields.join(", ")
    }
}

pub(crate) fn normalize_llm_patch(patch: &LlmConfigPatch) -> LlmConfigPatch {
    let provider = patch
        .provider
        .as_deref()
        .and_then(normalize_lowercase_non_empty_string);
    let base_url = patch.base_url.as_deref().and_then(normalize_provider_url);
    let provider_urls = patch
        .provider_urls
        .as_ref()
        .map(|urls| normalize_provider_url_entries(urls.as_slice()));
    let model = patch.model.as_deref().and_then(normalize_non_empty_string);
    let api_keys = patch
        .api_keys
        .as_ref()
        .map(|keys| normalize_string_entries_preserve_order(keys.as_slice()));
    let headers = patch.headers.as_ref().map(normalize_headers_map);
    let timeout_seconds = patch.timeout_seconds.filter(|value| *value > 0);
    let active_provider_id = patch
        .active_provider_id
        .as_deref()
        .and_then(normalize_non_empty_string);
    let providers = patch
        .providers
        .as_ref()
        .map(|providers| normalize_managed_providers(providers.as_slice()))
        .map(|providers| {
            providers
                .into_iter()
                .filter(|provider| provider.id.is_some() || provider.base_url.is_some())
                .collect::<Vec<_>>()
        });

    LlmConfigPatch {
        enabled: patch.enabled,
        provider,
        base_url,
        provider_urls,
        model,
        api_keys,
        timeout_seconds,
        headers,
        active_provider_id,
        providers,
    }
}

fn normalize_managed_providers(
    providers: &[LlmManagedProviderConfig],
) -> Vec<LlmManagedProviderConfig> {
    providers
        .iter()
        .map(|provider| {
            let id = provider.id.as_deref().and_then(normalize_non_empty_string);
            let label = provider
                .label
                .as_deref()
                .and_then(normalize_non_empty_string);
            let provider_id = provider
                .provider
                .as_deref()
                .and_then(normalize_lowercase_non_empty_string);
            let base_url = provider
                .base_url
                .as_deref()
                .and_then(normalize_provider_url);
            let api_key = provider
                .api_key
                .as_deref()
                .and_then(normalize_non_empty_string);
            let timeout_seconds = provider.timeout_seconds.filter(|value| *value > 0);
            let headers = provider.headers.as_ref().map(normalize_headers_map);
            let models = provider
                .models
                .as_ref()
                .map(|models| normalize_managed_models(models.as_slice()));

            LlmManagedProviderConfig {
                id,
                label,
                provider: provider_id,
                base_url,
                api_key,
                timeout_seconds,
                headers,
                models,
            }
        })
        .collect()
}

fn normalize_managed_models(models: &[LlmManagedModelConfig]) -> Vec<LlmManagedModelConfig> {
    models
        .iter()
        .filter_map(|model| {
            let id = model.id.as_deref().and_then(normalize_non_empty_string);
            id.map(|id| LlmManagedModelConfig {
                id: Some(id),
                enabled: Some(model.enabled.unwrap_or(false)),
            })
        })
        .collect()
}
