pub(crate) const APP_CONFIG_FILENAMES: [&str; 6] = [
    "config.yaml",
    "rust-config.yaml",
    "rust-config.yml",
    "rust-config.toml",
    "config/rust-core.yaml",
    "config/rust-core.toml",
];
pub(crate) const LLM_CONFIG_FILENAMES: [&str; 2] = ["llm-config.yaml", "llm-config.toml"];
pub(crate) const PASSWORD_CONFIG_FILENAME: &str = "password.yaml";
pub(crate) const WEBUI_PASSWORD_FILENAME: &str = "password.json";
pub(crate) const LLM_PROMPT_STORE_FILENAME: &str = "llm-prompts.json";
pub(crate) const MCP_CONFIG_FILENAME: &str = "mcp-servers.json";
pub(crate) const TOOL_STATE_FILENAME: &str = "tool-state.json";
pub(crate) const PLUGIN_CRON_STATE_FILENAME: &str = "plugin-cron-state.json";
pub(crate) const FLOW_LOCAL_AGENT_DEVICE_ID_FILENAME: &str = "flow-local-agent-device-id";
pub(crate) const SKILLS_DIR_NAME: &str = "skills";
pub(crate) const LEGACY_WEBUI_PASSWORD_RELATIVE_PATH: &str = ".liteyuki/password.json";
pub(crate) const LEGACY_LLM_PROMPT_STORE_PATH: &str = "llm-prompts.json";
