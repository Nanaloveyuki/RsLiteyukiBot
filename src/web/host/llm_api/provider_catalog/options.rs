use serde_json::{Value, json};

use crate::app_config::LlmManagedProviderConfig;
use crate::llm::service::{current_llm_runtime_config, resolve_provider_id};

use super::super::provider_state::load_managed_providers_from_doc;
use super::metadata::{model_options_for_provider, normalize_base_url, provider_label};

pub(in super::super) fn configured_provider_options(
    active_base_url: &str,
    active_provider_id: &str,
) -> Result<Vec<Value>, String> {
    let doc = crate::llm::service::load_current_app_config_doc()?;
    let runtime = current_llm_runtime_config()?;
    let managed_providers = load_managed_providers_from_doc(&doc, &runtime);
    let mut urls = doc
        .llm
        .as_ref()
        .and_then(|section| section.provider_urls.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|url| normalize_base_url(url.as_str()))
        .filter(|url| !url.is_empty())
        .collect::<Vec<_>>();
    if urls.is_empty() {
        urls.push(active_base_url.to_string());
    }
    for provider_url in managed_providers
        .iter()
        .filter_map(|provider| provider.base_url.as_deref())
        .map(normalize_base_url)
        .filter(|url| !url.is_empty())
    {
        if !urls.iter().any(|url| url == &provider_url) {
            urls.push(provider_url);
        }
    }
    if !active_base_url.is_empty() && !urls.iter().any(|url| url == active_base_url) {
        urls.push(active_base_url.to_string());
    }

    Ok(urls
        .into_iter()
        .enumerate()
        .map(|(index, url)| {
            let saved_provider = managed_providers
                .iter()
                .find(|provider| provider.base_url.as_deref() == Some(url.as_str()));
            let saved_provider_id =
                saved_provider.and_then(|provider| provider.provider.as_deref());
            let saved_label = saved_provider
                .and_then(|provider| provider.label.as_deref())
                .map(str::trim)
                .filter(|label| !label.is_empty());
            let option_provider_id = if let Some(saved_provider_id) = saved_provider_id {
                resolve_provider_id(Some(saved_provider_id), Some(url.as_str()))
            } else if url == active_base_url {
                resolve_provider_id(Some(active_provider_id), Some(url.as_str()))
            } else {
                resolve_provider_id(None, Some(url.as_str()))
            };
            let label = saved_label
                .map(ToString::to_string)
                .unwrap_or_else(|| provider_label(option_provider_id.as_str()));
            let model_options =
                managed_provider_model_options(saved_provider, option_provider_id.as_str());
            json!({
                "id": format!("{option_provider_id}-{index}"),
                "label": label,
                "baseUrl": url,
                "active": url == active_base_url,
                "modelOptions": model_options,
            })
        })
        .collect())
}

pub(in super::super) fn managed_provider_model_options(
    provider: Option<&LlmManagedProviderConfig>,
    provider_id: &str,
) -> Vec<String> {
    if let Some(provider) = provider
        && let Some(models) = provider.models.as_ref()
    {
        let enabled = models
            .iter()
            .filter(|model| model.enabled.unwrap_or(false))
            .filter_map(|model| model.id.as_deref())
            .map(str::trim)
            .filter(|model_id| !model_id.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if !enabled.is_empty() || !models.is_empty() {
            return enabled;
        }
    }

    model_options_for_provider(provider_id)
}
