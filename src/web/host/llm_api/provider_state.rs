use std::collections::{HashMap, HashSet};

use super::{WebLlmManagedProviderPayload, normalize_base_url, provider_label};

use crate::app_config::{
    AppConfigDoc, LlmManagedModelConfig, LlmManagedProviderConfig, LlmRuntimeConfig,
};
use crate::llm::service::resolve_provider_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManagedProviderModelView {
    pub(super) id: String,
    pub(super) enabled: bool,
}

pub(super) fn load_managed_providers_from_doc(
    doc: &AppConfigDoc,
    runtime: &LlmRuntimeConfig,
) -> Vec<LlmManagedProviderConfig> {
    if let Some(saved) = doc
        .llm
        .as_ref()
        .and_then(|section| section.providers.clone())
        .filter(|providers| !providers.is_empty())
    {
        let fallback_timeout = doc
            .llm
            .as_ref()
            .and_then(|section| section.timeout_seconds)
            .filter(|value| *value > 0)
            .unwrap_or_else(|| runtime.timeout_ms.saturating_div(1000).max(1));
        return saved
            .iter()
            .enumerate()
            .map(|(index, provider)| {
                enrich_saved_provider(provider, index, fallback_timeout, runtime)
            })
            .collect();
    }

    vec![LlmManagedProviderConfig {
        id: Some(derive_provider_entry_id(
            None,
            Some(runtime.provider.as_str()),
            runtime.base_url.as_str(),
            0,
        )),
        label: Some(provider_label(
            resolve_provider_id(
                Some(runtime.provider.as_str()),
                Some(runtime.base_url.as_str()),
            )
            .as_str(),
        )),
        provider: Some(resolve_provider_id(
            Some(runtime.provider.as_str()),
            Some(runtime.base_url.as_str()),
        )),
        base_url: Some(normalize_base_url(runtime.base_url.as_str())),
        api_key: runtime.api_keys.first().cloned(),
        timeout_seconds: Some(runtime.timeout_ms.saturating_div(1000).max(1)),
        headers: Some(runtime.headers.clone()),
        models: Some(vec![LlmManagedModelConfig {
            id: Some(runtime.model.clone()),
            enabled: Some(true),
        }]),
    }]
}

pub(super) fn resolve_active_provider_id(
    explicit_active_provider_id: Option<String>,
    providers: &[LlmManagedProviderConfig],
    active_base_url: &str,
) -> Option<String> {
    let explicit_active_provider_id = explicit_active_provider_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(explicit_active_provider_id) = explicit_active_provider_id
        && providers
            .iter()
            .any(|provider| provider.id.as_deref() == Some(explicit_active_provider_id))
    {
        return Some(explicit_active_provider_id.to_string());
    }

    providers
        .iter()
        .find(|provider| provider.base_url.as_deref() == Some(active_base_url))
        .and_then(|provider| provider.id.clone())
        .or_else(|| providers.first().and_then(|provider| provider.id.clone()))
}

pub(super) fn normalize_single_provider_input(
    provider: &WebLlmManagedProviderPayload,
    index: usize,
    fallback_timeout_seconds: u64,
) -> LlmManagedProviderConfig {
    let base_url = normalize_base_url(provider.base_url.as_str());
    let provider_id = provider
        .provider_id
        .as_deref()
        .or(Some(base_url.as_str()))
        .map(|_| resolve_provider_id(provider.provider_id.as_deref(), Some(base_url.as_str())))
        .unwrap_or_else(|| "openai-compatible".to_string());
    let id = derive_provider_entry_id(
        Some(provider.id.as_str()),
        Some(provider.label.as_str()),
        base_url.as_str(),
        index,
    );
    let label =
        normalize_optional_string(Some(provider.label.as_str())).unwrap_or_else(|| id.clone());
    let mut headers = provider
        .headers
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if key.is_empty() || value.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect::<HashMap<_, _>>();
    if headers.is_empty() {
        headers = HashMap::new();
    }
    let models = provider
        .models
        .iter()
        .filter_map(|model| {
            let id = model.id.trim();
            if id.is_empty() {
                None
            } else {
                Some(LlmManagedModelConfig {
                    id: Some(id.to_string()),
                    enabled: Some(model.enabled),
                })
            }
        })
        .collect::<Vec<_>>();

    LlmManagedProviderConfig {
        id: Some(id),
        label: Some(label),
        provider: Some(provider_id),
        base_url: Some(base_url),
        api_key: normalize_optional_string(Some(provider.api_key.as_str())),
        timeout_seconds: Some(
            provider
                .timeout_seconds
                .unwrap_or(fallback_timeout_seconds.max(1))
                .max(1),
        ),
        headers: Some(headers),
        models: Some(models),
    }
}

pub(super) fn normalize_managed_provider_inputs(
    providers: &[WebLlmManagedProviderPayload],
    fallback_timeout_seconds: u64,
) -> Vec<LlmManagedProviderConfig> {
    providers
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            normalize_single_provider_input(provider, index, fallback_timeout_seconds)
        })
        .collect()
}

pub(super) fn provider_enabled_model_set(provider: &LlmManagedProviderConfig) -> HashSet<String> {
    provider
        .models
        .as_ref()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            model
                .enabled
                .unwrap_or(false)
                .then(|| model.id.clone())
                .flatten()
        })
        .collect()
}

pub(super) fn resolve_models_to_test(
    provider: &LlmManagedProviderConfig,
    explicit_model_id: Option<&str>,
    test_all: bool,
) -> Vec<String> {
    if let Some(explicit_model_id) = explicit_model_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return vec![explicit_model_id.to_string()];
    }

    if test_all {
        return provider
            .models
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|model| model.id.clone())
            .collect();
    }

    select_primary_model_id(provider).into_iter().collect()
}

pub(super) fn merge_saved_and_discovered_models(
    provider: &LlmManagedProviderConfig,
    discovered_models: &[String],
    enabled_models: &HashSet<String>,
) -> Vec<ManagedProviderModelView> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();

    for model_id in discovered_models {
        if seen.insert(model_id.clone()) {
            merged.push(ManagedProviderModelView {
                id: model_id.clone(),
                enabled: enabled_models.contains(model_id),
            });
        }
    }

    if let Some(saved_models) = provider.models.as_ref() {
        for saved_model in saved_models {
            let Some(model_id) = saved_model.id.as_ref() else {
                continue;
            };
            if seen.insert(model_id.clone()) {
                merged.push(ManagedProviderModelView {
                    id: model_id.clone(),
                    enabled: saved_model.enabled.unwrap_or(false),
                });
            }
        }
    }

    merged
}

pub(super) fn select_primary_model_id(provider: &LlmManagedProviderConfig) -> Option<String> {
    provider
        .models
        .as_ref()
        .and_then(|models| {
            models
                .iter()
                .find(|model| model.enabled.unwrap_or(false))
                .or_else(|| models.first())
        })
        .and_then(|model| model.id.clone())
}

fn enrich_saved_provider(
    provider: &LlmManagedProviderConfig,
    index: usize,
    fallback_timeout_seconds: u64,
    runtime: &LlmRuntimeConfig,
) -> LlmManagedProviderConfig {
    let base_url = provider
        .base_url
        .as_deref()
        .map(normalize_base_url)
        .filter(|value| !value.is_empty())
        .or_else(|| Some(normalize_base_url(runtime.base_url.as_str())));
    let provider_id = provider
        .provider
        .as_deref()
        .or(base_url.as_deref())
        .map(|_| resolve_provider_id(provider.provider.as_deref(), base_url.as_deref()));
    let id = Some(derive_provider_entry_id(
        provider.id.as_deref(),
        provider.label.as_deref(),
        base_url.as_deref().unwrap_or(runtime.base_url.as_str()),
        index,
    ));
    let label = normalize_optional_string(provider.label.as_deref())
        .or_else(|| id.clone())
        .or_else(|| provider_id.as_deref().map(provider_label));
    let api_key = normalize_optional_string(provider.api_key.as_deref()).or_else(|| {
        if base_url.as_deref() == Some(runtime.base_url.as_str()) {
            runtime.api_keys.first().cloned()
        } else {
            None
        }
    });
    let headers = provider.headers.clone().unwrap_or_else(|| {
        if base_url.as_deref() == Some(runtime.base_url.as_str()) {
            runtime.headers.clone()
        } else {
            HashMap::new()
        }
    });
    let models = provider
        .models
        .clone()
        .filter(|models| !models.is_empty())
        .or_else(|| {
            if base_url.as_deref() == Some(runtime.base_url.as_str()) {
                Some(vec![LlmManagedModelConfig {
                    id: Some(runtime.model.clone()),
                    enabled: Some(true),
                }])
            } else {
                None
            }
        });

    LlmManagedProviderConfig {
        id,
        label,
        provider: provider_id,
        base_url,
        api_key,
        timeout_seconds: provider
            .timeout_seconds
            .filter(|value| *value > 0)
            .or(Some(fallback_timeout_seconds.max(1))),
        headers: Some(headers),
        models,
    }
}

fn derive_provider_entry_id(
    explicit_id: Option<&str>,
    label: Option<&str>,
    base_url: &str,
    index: usize,
) -> String {
    let raw = explicit_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| label.map(str::trim).filter(|value| !value.is_empty()))
        .unwrap_or(base_url);
    let slug = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        format!("provider-{}", index + 1)
    } else {
        slug
    }
}

fn normalize_optional_string(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}
