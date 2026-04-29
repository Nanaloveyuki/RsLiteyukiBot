use std::path::PathBuf;

use crate::app_config::{
    AppConfigDoc, LlmConfigSection, load_app_config_from_path, load_app_config_with_warnings,
    validate_app_config,
};
use crate::hardcode_data::llm::default_llm_config_template;
use crate::i18n::trf;
use crate::utils::config_path::{
    migrate_legacy_llm_config_to_user_dir, replace_config_file, resolve_default_llm_config_path,
    resolve_existing_legacy_llm_config_path, resolve_existing_llm_config_path,
    resolve_existing_user_llm_config_path,
};

use super::dedup_warnings;

pub(crate) fn ensure_default_llm_config_file() -> Result<(), String> {
    if let Ok(path) = std::env::var("LY_LLM_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return ensure_llm_config_file(std::path::Path::new(path.trim()));
    }

    if let Some(user_path) = resolve_existing_user_llm_config_path() {
        replace_default_user_llm_config_with_legacy(user_path.as_path())?;
        return Ok(());
    }

    migrate_legacy_llm_config_to_user_dir()?;

    if resolve_existing_user_llm_config_path().is_some() {
        return Ok(());
    }

    let path = resolve_default_llm_config_path();
    ensure_llm_config_file(path.as_path())
}

pub(crate) fn ensure_llm_config_file(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create llm config parent directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let template = default_llm_config_template(path);
    std::fs::write(path, template)
        .map_err(|err| format!("failed to write llm config {}: {err}", path.display()))?;
    Ok(())
}

fn replace_default_user_llm_config_with_legacy(user_path: &std::path::Path) -> Result<(), String> {
    let Some(legacy_path) = resolve_existing_legacy_llm_config_path() else {
        return Ok(());
    };
    if legacy_path == user_path {
        return Ok(());
    }

    let current = std::fs::read_to_string(user_path).map_err(|err| {
        format!(
            "failed to read user llm config {}: {err}",
            user_path.display()
        )
    })?;
    let template = default_llm_config_template(user_path);
    if current.trim() != template.trim() {
        return Ok(());
    }

    replace_config_file(legacy_path.as_path(), user_path)
}

pub(crate) fn load_app_config_with_llm_overlay() -> (AppConfigDoc, Vec<String>) {
    let (mut app_config, mut warnings) = load_app_config_with_warnings(false);
    if let Some(path) = resolve_llm_config_path() {
        match load_app_config_from_path(path.as_path()) {
            Ok(overlay_doc) => {
                if let Some(overlay_llm) = overlay_doc.llm {
                    app_config.llm = Some(merge_llm_config_sections(
                        app_config.llm.take(),
                        overlay_llm,
                    ));
                }
            }
            Err(err) => {
                let path_display = path.display().to_string();
                let err_text = err.to_string();
                warnings.push(
                    trf(
                        "startup.llm_overlay_load_failed",
                        &[("path", path_display.as_str()), ("err", err_text.as_str())],
                    )
                    .to_string(),
                );
            }
        }
    }
    warnings.extend(validate_app_config(&app_config));
    warnings = dedup_warnings(warnings);
    (app_config, warnings)
}

pub(crate) fn merge_llm_config_sections(
    base: Option<LlmConfigSection>,
    overlay: LlmConfigSection,
) -> LlmConfigSection {
    let mut merged = base.unwrap_or_default();
    if overlay.enabled.is_some() {
        merged.enabled = overlay.enabled;
    }
    if overlay.stream.is_some() {
        merged.stream = overlay.stream;
    }
    if overlay.provider.is_some() {
        merged.provider = overlay.provider;
    }
    if overlay.base_url.is_some() {
        merged.base_url = overlay.base_url;
    }
    if overlay.provider_urls.is_some() {
        merged.provider_urls = overlay.provider_urls;
    }
    if overlay.api_keys.is_some() {
        merged.api_keys = overlay.api_keys;
    }
    if overlay.api_key.is_some() {
        merged.api_key = overlay.api_key;
    }
    if overlay.headers.is_some() {
        merged.headers = overlay.headers;
    }
    if overlay.model.is_some() {
        merged.model = overlay.model;
    }
    if overlay.timeout_seconds.is_some() {
        merged.timeout_seconds = overlay.timeout_seconds;
    }
    if overlay.temperature.is_some() {
        merged.temperature = overlay.temperature;
    }
    if overlay.top_p.is_some() {
        merged.top_p = overlay.top_p;
    }
    if overlay.top_k.is_some() {
        merged.top_k = overlay.top_k;
    }
    if overlay.frequency_penalty.is_some() {
        merged.frequency_penalty = overlay.frequency_penalty;
    }
    if overlay.presence_penalty.is_some() {
        merged.presence_penalty = overlay.presence_penalty;
    }
    if overlay.parallel_tool_calls.is_some() {
        merged.parallel_tool_calls = overlay.parallel_tool_calls;
    }
    if overlay.system_prompt.is_some() {
        merged.system_prompt = overlay.system_prompt;
    }
    if overlay.command_prefix.is_some() {
        merged.command_prefix = overlay.command_prefix;
    }
    if overlay.active_provider_id.is_some() {
        merged.active_provider_id = overlay.active_provider_id;
    }
    if overlay.providers.is_some() {
        merged.providers = overlay.providers;
    }
    merged
}

pub(crate) fn resolve_llm_config_path() -> Option<PathBuf> {
    resolve_existing_llm_config_path()
}
