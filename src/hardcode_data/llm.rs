pub(crate) const DEFAULT_LLM_PROVIDER: &str = "openai";
pub(crate) const DEFAULT_LLM_BASE_URL: &str = "https://api.openai.com";
pub(crate) const DEFAULT_LLM_MODEL: &str = "gpt-4.1-mini";
pub(crate) const DEFAULT_LLM_TIMEOUT_SECONDS: u64 = 20;
pub(crate) const DEFAULT_LLM_COMMAND_PREFIX: &str = "/ask";

pub(crate) fn default_llm_config_template(path: &std::path::Path) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("toml") => format!(
            "[llm]\nenabled = false\nprovider = \"{DEFAULT_LLM_PROVIDER}\"\nbase_url = \"{DEFAULT_LLM_BASE_URL}\"\nmodel = \"{DEFAULT_LLM_MODEL}\"\ntimeout_seconds = {DEFAULT_LLM_TIMEOUT_SECONDS}\ncommand_prefix = \"{DEFAULT_LLM_COMMAND_PREFIX}\"\napi_keys = []\n"
        ),
        _ => format!(
            "llm:\n  enabled: false\n  provider: {DEFAULT_LLM_PROVIDER}\n  base_url: {DEFAULT_LLM_BASE_URL}\n  model: {DEFAULT_LLM_MODEL}\n  timeout_seconds: {DEFAULT_LLM_TIMEOUT_SECONDS}\n  command_prefix: {DEFAULT_LLM_COMMAND_PREFIX}\n  api_keys: []\n"
        ),
    }
}
