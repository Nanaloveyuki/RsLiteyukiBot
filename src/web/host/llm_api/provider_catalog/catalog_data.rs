use serde_json::Value;

pub(in super::super) fn provider_catalog_entry(provider_id: &str) -> Option<Value> {
    crate::hardcode_data::llm_provider_catalog::provider_catalog()
        .into_iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(provider_id))
}

pub(in super::super) fn provider_catalog_label(provider_id: &str) -> Option<String> {
    provider_catalog_entry(provider_id).and_then(|entry| {
        entry
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

pub(in super::super) fn provider_catalog_sample_models(provider_id: &str) -> Vec<String> {
    provider_catalog_entry(provider_id)
        .and_then(|entry| entry.get("sampleModels").cloned())
        .and_then(|models| {
            models.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
        })
        .unwrap_or_default()
}

pub(in super::super) fn provider_catalog_parameter_support(
    provider_id: &str,
) -> Option<serde_json::Map<String, Value>> {
    provider_catalog_entry(provider_id)
        .and_then(|entry| entry.get("parameterSupport").cloned())
        .and_then(|value| value.as_object().cloned())
}
