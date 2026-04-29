use super::access::{config_i18n, config_llm, config_tui_resume};
use super::*;
use crate::hardcode_data::llm::{
    DEFAULT_LLM_BASE_URL, DEFAULT_LLM_COMMAND_PREFIX, DEFAULT_LLM_MODEL, DEFAULT_LLM_PROVIDER,
    DEFAULT_LLM_TIMEOUT_SECONDS,
};
use crate::utils::llm_config::{
    collect_llm_api_keys, normalize_headers_map, normalize_lowercase_non_empty_string,
    normalize_non_empty_string, normalize_provider_url, normalize_provider_url_entries,
    parse_llm_key_list,
};

pub(crate) fn resolve_tui_config(app_config: &AppConfigDoc) -> tui::TuiConfig {
    let mut config = tui::TuiConfig::default();

    if let Some(resume) = config_tui_resume(app_config) {
        if let Some(path) = resume.store_path.as_deref() {
            config.resume_store_path = PathBuf::from(path);
        }
        if let Some(max_sessions) = resume.max_sessions
            && max_sessions > 0
        {
            config.resume_max_sessions = max_sessions;
        }
        if let Some(max_size_mib) = resume.max_size_mib
            && max_size_mib > 0
        {
            config.resume_max_size_mib = max_size_mib;
        }
    }

    if let Ok(path) = std::env::var("LY_RESUME_STORE_PATH") {
        config.resume_store_path = PathBuf::from(path);
    }
    if let Ok(raw) = std::env::var("LY_TUI_RESUME_MAX_SESSIONS")
        && let Ok(value) = raw.trim().parse::<usize>()
        && value > 0
    {
        config.resume_max_sessions = value;
    }
    if let Ok(raw) = std::env::var("LY_TUI_RESUME_MAX_SIZE_MIB")
        && let Ok(value) = raw.trim().parse::<u64>()
        && value > 0
    {
        config.resume_max_size_mib = value;
    }

    config
}

pub(crate) fn resolve_app_locale(app_config: &AppConfigDoc) -> AppLocale {
    let mut locale = config_i18n(app_config)
        .and_then(|section| section.locale.as_deref())
        .and_then(AppLocale::parse)
        .unwrap_or_default();

    if let Ok(raw) = std::env::var("LY_LOCALE")
        && let Some(parsed) = AppLocale::parse(raw.as_str())
    {
        locale = parsed;
    }

    locale
}

pub(crate) fn resolve_llm_config(app_config: &AppConfigDoc) -> LlmRuntimeConfig {
    let section = config_llm(app_config);

    let enabled = std::env::var("LY_LLM_ENABLED")
        .ok()
        .and_then(|raw| super::parse_bool_env(raw.trim()))
        .or_else(|| section.and_then(|cfg| cfg.enabled))
        .unwrap_or(false);

    let stream = std::env::var("LY_LLM_STREAM")
        .ok()
        .and_then(|raw| super::parse_bool_env(raw.trim()))
        .or_else(|| section.and_then(|cfg| cfg.stream))
        .unwrap_or(false);

    let provider = std::env::var("LY_LLM_PROVIDER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| section.and_then(|cfg| cfg.provider.clone()))
        .unwrap_or_else(|| DEFAULT_LLM_PROVIDER.to_string());
    let provider = normalize_lowercase_non_empty_string(provider.as_str())
        .unwrap_or_else(|| DEFAULT_LLM_PROVIDER.to_string());

    let provider_urls = section
        .and_then(|cfg| cfg.provider_urls.clone())
        .map(|urls| normalize_provider_url_entries(urls.as_slice()))
        .unwrap_or_default();

    let base_url = std::env::var("LY_LLM_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| section.and_then(|cfg| cfg.base_url.clone()))
        .or_else(|| provider_urls.first().cloned())
        .unwrap_or_else(|| DEFAULT_LLM_BASE_URL.to_string());
    let base_url = normalize_provider_url(base_url.as_str())
        .unwrap_or_else(|| DEFAULT_LLM_BASE_URL.to_string());

    let mut api_keys = std::env::var("LY_LLM_API_KEYS")
        .ok()
        .map(|raw| parse_llm_key_list(raw.as_str()))
        .unwrap_or_default();
    if api_keys.is_empty()
        && let Some(section_keys) = section.and_then(|cfg| cfg.api_keys.clone())
    {
        api_keys = collect_llm_api_keys(Some(&section_keys), None);
    }

    let single_api_key = std::env::var("LY_LLM_API_KEY")
        .ok()
        .or_else(|| section.and_then(|cfg| cfg.api_key.clone()))
        .and_then(|value| normalize_non_empty_string(value.as_str()));
    if let Some(single_api_key) = single_api_key {
        api_keys.push(single_api_key);
    }
    api_keys = collect_llm_api_keys(Some(&api_keys), None);

    let headers = section
        .and_then(|cfg| cfg.headers.clone())
        .map(|headers| normalize_headers_map(&headers))
        .unwrap_or_default();

    let model = std::env::var("LY_LLM_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| section.and_then(|cfg| cfg.model.clone()))
        .unwrap_or_else(|| DEFAULT_LLM_MODEL.to_string());
    let model =
        normalize_non_empty_string(model.as_str()).unwrap_or_else(|| DEFAULT_LLM_MODEL.to_string());

    let timeout_seconds = std::env::var("LY_LLM_TIMEOUT_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .or_else(|| {
            section
                .and_then(|cfg| cfg.timeout_seconds)
                .filter(|value| *value > 0)
        })
        .unwrap_or(DEFAULT_LLM_TIMEOUT_SECONDS);
    let timeout_ms = super::seconds_to_timeout_ms(Some(timeout_seconds));

    let temperature = std::env::var("LY_LLM_TEMPERATURE")
        .ok()
        .and_then(|raw| raw.trim().parse::<f32>().ok())
        .or_else(|| section.and_then(|cfg| cfg.temperature))
        .filter(|value| value.is_finite() && (0.0..=2.0).contains(value));

    let top_p = std::env::var("LY_LLM_TOP_P")
        .ok()
        .and_then(|raw| raw.trim().parse::<f32>().ok())
        .or_else(|| section.and_then(|cfg| cfg.top_p))
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value));

    let top_k = std::env::var("LY_LLM_TOP_K")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .or_else(|| section.and_then(|cfg| cfg.top_k))
        .filter(|value| *value > 0);
    let frequency_penalty = std::env::var("LY_LLM_FREQUENCY_PENALTY")
        .ok()
        .and_then(|raw| raw.trim().parse::<f32>().ok())
        .or_else(|| section.and_then(|cfg| cfg.frequency_penalty))
        .filter(|value| value.is_finite() && (-2.0..=2.0).contains(value));
    let presence_penalty = std::env::var("LY_LLM_PRESENCE_PENALTY")
        .ok()
        .and_then(|raw| raw.trim().parse::<f32>().ok())
        .or_else(|| section.and_then(|cfg| cfg.presence_penalty))
        .filter(|value| value.is_finite() && (-2.0..=2.0).contains(value));

    let parallel_tool_calls = std::env::var("LY_LLM_PARALLEL_TOOL_CALLS")
        .ok()
        .and_then(|raw| super::parse_bool_env(raw.trim()))
        .or_else(|| section.and_then(|cfg| cfg.parallel_tool_calls))
        .unwrap_or(true);

    let system_prompt = std::env::var("LY_LLM_SYSTEM_PROMPT")
        .ok()
        .or_else(|| section.and_then(|cfg| cfg.system_prompt.clone()))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let command_prefix = std::env::var("LY_LLM_COMMAND_PREFIX")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| section.and_then(|cfg| cfg.command_prefix.clone()))
        .unwrap_or_else(|| DEFAULT_LLM_COMMAND_PREFIX.to_string());
    let command_prefix = normalize_non_empty_string(command_prefix.as_str())
        .unwrap_or_else(|| DEFAULT_LLM_COMMAND_PREFIX.to_string());

    LlmRuntimeConfig {
        enabled,
        stream,
        provider,
        base_url,
        api_keys,
        headers,
        model,
        timeout_ms,
        temperature,
        top_p,
        top_k,
        frequency_penalty,
        presence_penalty,
        parallel_tool_calls,
        system_prompt,
        command_prefix,
    }
}
