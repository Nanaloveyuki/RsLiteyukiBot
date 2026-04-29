use std::path::PathBuf;

use crate::app_config::{AppConfigDoc, LlmRuntimeConfig};
use crate::config_edit;
use crate::hardcode_data::llm::DEFAULT_LLM_BASE_URL;
use crate::i18n::{tr, trf};
use crate::llm::service::{
    current_active_prompt_profile, current_llm_runtime_config, generate_llm_reply,
    load_current_app_config_doc, load_llm_prompt_store, persist_llm_prompt_store,
    probe_llm_runtime_text, resolve_llm_prompt_store_path, resolve_runtime_provider_id,
};
use crate::llm::{LlmPromptPreview, build_prompt_preview};
use crate::runtime_support::{ensure_llm_config_file, resolve_llm_config_path};
use crate::tui;
use crate::utils::config_path::resolve_default_llm_config_path;
use crate::utils::llm_config::{
    collect_llm_api_keys, normalize_provider_url, normalize_provider_url_entries,
};

pub(crate) fn handle_llm_tui_command(
    action: tui::LlmCommandRequest,
) -> tui::LlmCommandFuture<'static> {
    Box::pin(async move {
        match action {
            tui::LlmCommandRequest::SetModel(model) => {
                let patch = config_edit::LlmConfigPatch {
                    model: Some(model.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(trf(
                    "llm.tui.model_updated",
                    &[
                        ("model", model.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::AddApiKeys(new_keys) => {
                let doc = load_current_app_config_doc()?;
                let mut merged = extract_llm_keys_from_doc(&doc);
                merged.extend(new_keys);
                merged = collect_llm_api_keys(Some(&merged), None);
                if merged.is_empty() {
                    return Err(tr("llm.tui.no_valid_api_key"));
                }

                let patch = config_edit::LlmConfigPatch {
                    api_keys: Some(merged.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(trf(
                    "llm.tui.api_keys_updated",
                    &[
                        ("count", merged.len().to_string().as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::ProbeProvider(provider_override) => {
                let mut llm_config = current_llm_runtime_config()?;
                if let Some(provider) = provider_override {
                    llm_config.provider = provider;
                }
                let message = probe_llm_configuration(&llm_config).await?;
                Ok(message)
            }
            tui::LlmCommandRequest::AddProviderUrl(provider_url) => {
                let doc = load_current_app_config_doc()?;
                let mut provider_urls = extract_llm_provider_urls_from_doc(&doc);
                if provider_urls.iter().any(|value| value == &provider_url) {
                    return Ok(trf(
                        "llm.tui.provider_exists",
                        &[("provider_url", provider_url.as_str())],
                    ));
                }

                provider_urls.push(provider_url.clone());
                provider_urls = normalize_provider_url_entries(provider_urls.as_slice());
                let active_base_url = configured_llm_base_url_from_doc(&doc)
                    .or_else(|| provider_urls.first().cloned())
                    .unwrap_or_else(|| DEFAULT_LLM_BASE_URL.to_string());
                let patch = config_edit::LlmConfigPatch {
                    base_url: Some(active_base_url.clone()),
                    provider_urls: Some(provider_urls.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(trf(
                    "llm.tui.provider_added",
                    &[
                        ("provider_url", provider_url.as_str()),
                        ("count", provider_urls.len().to_string().as_str()),
                        ("active_base_url", active_base_url.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::RemoveProviderUrl(provider_url) => {
                let doc = load_current_app_config_doc()?;
                let mut provider_urls = extract_llm_provider_urls_from_doc(&doc);
                ensure_registered_provider_url(provider_url.as_str(), &provider_urls)?;
                provider_urls.retain(|value| value != &provider_url);

                let configured_base_url = configured_llm_base_url_from_doc(&doc);
                let active_base_url = if configured_base_url
                    .as_ref()
                    .is_some_and(|base| base == &provider_url)
                {
                    provider_urls.first().cloned()
                } else {
                    configured_base_url.or_else(|| provider_urls.first().cloned())
                }
                .unwrap_or_else(|| DEFAULT_LLM_BASE_URL.to_string());

                let patch = config_edit::LlmConfigPatch {
                    base_url: Some(active_base_url.clone()),
                    provider_urls: Some(provider_urls.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(trf(
                    "llm.tui.provider_removed",
                    &[
                        ("provider_url", provider_url.as_str()),
                        ("count", provider_urls.len().to_string().as_str()),
                        ("active_base_url", active_base_url.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::ListProviderUrls => {
                let doc = load_current_app_config_doc()?;
                let llm_config = current_llm_runtime_config()?;
                let provider_urls = extract_llm_provider_urls_from_doc(&doc);
                let path = resolve_llm_config_path().unwrap_or_else(resolve_llm_config_write_path);
                let mut lines = vec![format!("* {}", llm_config.base_url)];
                lines.extend(
                    provider_urls
                        .iter()
                        .filter(|url| **url != llm_config.base_url)
                        .map(|url| format!("  {url}")),
                );
                Ok(trf(
                    "llm.tui.provider_list",
                    &[
                        ("path", path.display().to_string().as_str()),
                        ("lines", lines.join("\n").as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::UseProviderUrl(provider_url) => {
                let doc = load_current_app_config_doc()?;
                let provider_urls = extract_llm_provider_urls_from_doc(&doc);
                ensure_registered_provider_url(provider_url.as_str(), &provider_urls)?;

                let patch = config_edit::LlmConfigPatch {
                    base_url: Some(provider_url.clone()),
                    provider_urls: Some(provider_urls.clone()),
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(trf(
                    "llm.tui.provider_switched",
                    &[
                        ("provider_url", provider_url.as_str()),
                        ("count", provider_urls.len().to_string().as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::SetEnabled { enabled, provider } => {
                let patch = config_edit::LlmConfigPatch {
                    enabled: Some(enabled),
                    provider,
                    ..Default::default()
                };
                let path = persist_llm_patch(&patch)?;
                Ok(trf(
                    "llm.tui.enabled_state",
                    &[
                        (
                            "state",
                            if enabled {
                                tr("llm.tui.state.enabled")
                            } else {
                                tr("llm.tui.state.disabled")
                            }
                            .as_str(),
                        ),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::PromptList => {
                let store = load_llm_prompt_store()?;
                let path = resolve_llm_prompt_store_path();
                let mut names = store.profile_names();
                names.sort();

                let lines = names
                    .into_iter()
                    .map(|name| {
                        if name == store.active_profile {
                            format!("* {name}")
                        } else {
                            format!("  {name}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                Ok(trf(
                    "llm.tui.prompt_profiles",
                    &[
                        ("path", path.display().to_string().as_str()),
                        ("lines", lines.as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::PromptUse(name) => {
                let mut store = load_llm_prompt_store()?;
                store.set_active_profile(name.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(trf(
                    "llm.tui.prompt_used",
                    &[
                        ("name", store.active_profile.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::PromptSet { name, soul } => {
                let mut store = load_llm_prompt_store()?;
                store.upsert_profile(name.as_str(), soul.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(trf(
                    "llm.tui.prompt_updated",
                    &[
                        ("name", name.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::PromptRemove(name) => {
                let mut store = load_llm_prompt_store()?;
                store.remove_profile(name.as_str())?;
                let path = persist_llm_prompt_store(&store)?;
                Ok(trf(
                    "llm.tui.prompt_removed",
                    &[
                        ("name", name.as_str()),
                        ("path", path.display().to_string().as_str()),
                    ],
                ))
            }
            tui::LlmCommandRequest::PromptPreview { user_prompt } => {
                let llm_config = current_llm_runtime_config()?;
                let profile = current_active_prompt_profile()?;
                let preview = build_prompt_preview(
                    llm_config.system_prompt.as_deref(),
                    user_prompt.as_str(),
                    profile.soul.as_str(),
                );
                Ok(format_prompt_preview(&profile.name, &preview))
            }
        }
    })
}

pub(crate) fn handle_tui_ask_command(prompt: String) -> tui::AskFuture<'static> {
    Box::pin(async move {
        let output = generate_llm_reply(&prompt).await?;
        if output.trim().is_empty() {
            Ok(tr("main.llm.empty"))
        } else {
            Ok(output)
        }
    })
}

fn persist_llm_patch(patch: &config_edit::LlmConfigPatch) -> Result<PathBuf, String> {
    let path = resolve_llm_config_write_path();
    ensure_llm_config_file(path.as_path())?;
    config_edit::persist_llm_config(path.as_path(), patch)?;
    Ok(path)
}

fn resolve_llm_config_write_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_LLM_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    resolve_default_llm_config_path()
}

fn extract_llm_keys_from_doc(doc: &AppConfigDoc) -> Vec<String> {
    doc.llm
        .as_ref()
        .map(|section| collect_llm_api_keys(section.api_keys.as_ref(), section.api_key.as_deref()))
        .unwrap_or_default()
}

fn extract_llm_provider_urls_from_doc(doc: &AppConfigDoc) -> Vec<String> {
    doc.llm
        .as_ref()
        .and_then(|section| section.provider_urls.as_ref())
        .map(|urls| normalize_provider_url_entries(urls.as_slice()))
        .unwrap_or_default()
}

fn configured_llm_base_url_from_doc(doc: &AppConfigDoc) -> Option<String> {
    doc.llm
        .as_ref()
        .and_then(|section| section.base_url.as_deref())
        .and_then(normalize_provider_url)
}

fn ensure_registered_provider_url(
    provider_url: &str,
    provider_urls: &[String],
) -> Result<(), String> {
    if provider_urls.is_empty() {
        return Err(tr("llm.tui.provider_none_configured"));
    }
    if provider_urls.iter().all(|value| value != provider_url) {
        return Err(trf(
            "llm.tui.provider_not_found",
            &[("provider_url", provider_url)],
        ));
    }
    Ok(())
}

async fn probe_llm_configuration(llm_config: &LlmRuntimeConfig) -> Result<String, String> {
    let Some(api_key) = llm_config.api_keys.first() else {
        return Ok(trf(
            "llm.tui.probe_skipped",
            &[
                ("provider", resolve_runtime_provider_id(llm_config).as_str()),
                ("base_url", llm_config.base_url.as_str()),
            ],
        ));
    };

    let output = probe_llm_runtime_text(llm_config, api_key, "Reply exactly with: OK").await?;
    let preview = truncate_text_for_log(output.trim(), 80);
    Ok(trf(
        "llm.tui.probe_success",
        &[
            ("provider", resolve_runtime_provider_id(llm_config).as_str()),
            ("model", llm_config.model.as_str()),
            ("output", preview.as_str()),
        ],
    ))
}

fn format_prompt_preview(profile_name: &str, preview: &LlmPromptPreview) -> String {
    let system_prompt = if preview.system_prompt.trim().is_empty() {
        tr("llm.tui.empty_value")
    } else {
        preview.system_prompt.clone()
    };
    let composed_user_prompt = if preview.composed_user_prompt.trim().is_empty() {
        tr("llm.tui.empty_value")
    } else {
        preview.composed_user_prompt.clone()
    };

    trf(
        "llm.tui.prompt_preview",
        &[
            ("profile", profile_name),
            ("system_prompt", system_prompt.as_str()),
            ("composed_user_prompt", composed_user_prompt.as_str()),
            ("combined_prompt", preview.combined_prompt.as_str()),
        ],
    )
}

fn truncate_text_for_log(raw: &str, max_chars: usize) -> String {
    let mut iter = raw.chars();
    let preview: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

#[cfg(test)]
#[path = "llm_tui_command_service/tests.rs"]
mod tests;
