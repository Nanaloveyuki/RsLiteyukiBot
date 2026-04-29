use serde_json::{Value, json};

use super::{ManagedProviderModelView, provider_label};

use crate::app_config::LlmManagedProviderConfig;
use crate::llm::service::resolve_provider_id;

pub(super) fn serialize_managed_provider(
    provider: &LlmManagedProviderConfig,
    active_provider_id: Option<&str>,
) -> Value {
    let provider_id = provider
        .provider
        .as_deref()
        .or(provider.base_url.as_deref())
        .map(|_| resolve_provider_id(provider.provider.as_deref(), provider.base_url.as_deref()))
        .unwrap_or_else(|| "openai-compatible".to_string());
    let label = provider
        .label
        .clone()
        .or_else(|| provider.id.clone())
        .unwrap_or_else(|| provider_label(provider_id.as_str()));
    let headers = provider.headers.clone().unwrap_or_default();
    let models = provider
        .models
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|model| {
            model.id.map(|id| {
                json!({
                    "id": id,
                    "enabled": model.enabled.unwrap_or(false),
                })
            })
        })
        .collect::<Vec<_>>();

    json!({
        "id": provider.id.clone().unwrap_or_default(),
        "label": label,
        "providerId": provider_id.clone(),
        "providerLabel": provider_label(provider_id.as_str()),
        "baseUrl": provider.base_url.clone().unwrap_or_default(),
        "apiKey": provider.api_key.clone().unwrap_or_default(),
        "timeoutSeconds": provider.timeout_seconds.unwrap_or(120),
        "headers": headers,
        "models": models,
        "active": active_provider_id.is_some_and(|active_id| provider.id.as_deref() == Some(active_id)),
    })
}

pub(super) fn serialize_managed_provider_models(models: &[ManagedProviderModelView]) -> Vec<Value> {
    models
        .iter()
        .map(|model| {
            json!({
                "id": model.id,
                "enabled": model.enabled,
            })
        })
        .collect()
}
