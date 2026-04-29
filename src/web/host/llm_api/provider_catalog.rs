#[path = "provider_catalog/catalog_data.rs"]
mod catalog_data;
#[path = "provider_catalog/metadata.rs"]
mod metadata;
#[path = "provider_catalog/options.rs"]
mod options;

pub(super) use self::catalog_data::{
    provider_catalog_label, provider_catalog_parameter_support, provider_catalog_sample_models,
};
pub(super) use self::metadata::{
    current_provider_supports, detect_provider_id, model_options_for_provider, normalize_base_url,
    provider_label, reasoning_options_for_provider,
};
pub(super) use self::options::{configured_provider_options, managed_provider_model_options};

pub(super) fn provider_catalog() -> Vec<serde_json::Value> {
    crate::hardcode_data::llm_provider_catalog::provider_catalog()
}
