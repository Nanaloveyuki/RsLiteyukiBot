use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::app_config::LlmConfigSection;
use crate::superuser::SuperuserManager;
use crate::tui;
use liteyukibot_core::AdapterConfig;
use liteyukibot_core::BotRuntimeConfig;

#[path = "runtime_support/bootstrap.rs"]
mod bootstrap;
#[path = "runtime_support/gateway.rs"]
mod gateway;
#[path = "runtime_support/llm_config.rs"]
mod llm_config;
#[path = "runtime_support/plugin_dirs.rs"]
mod plugin_dirs;

pub(crate) use self::bootstrap::{
    dedup_warnings, describe_runtime_config, prepare_runtime_bootstrap,
};
pub(crate) use self::gateway::{ExternalGateway, ExternalGatewaySnapshot, next_llm_api_key_index};
pub(crate) use self::llm_config::{
    ensure_default_llm_config_file, ensure_llm_config_file, load_app_config_with_llm_overlay,
    resolve_llm_config_path,
};
pub(crate) use self::plugin_dirs::resolve_builtin_plugin_dirs;

pub(crate) const EXTERNAL_API_TIMEOUT: Duration = Duration::from_secs(12);
pub(crate) const BUILTIN_PLUGIN_DIRS: [&str; 2] = ["builtin_plugin", "resources/builtin_plugin"];
pub(crate) const DEV_BUILTIN_PLUGIN_DIRS: [&str; 1] = ["src/builtin_plugin"];

pub(crate) struct PreparedRuntimeBootstrap {
    pub(crate) warnings: Vec<String>,
    pub(crate) runtime_config: BotRuntimeConfig,
    pub(crate) effective_runtime_config: BotRuntimeConfig,
    pub(crate) adapter_configs: Vec<AdapterConfig>,
    pub(crate) adapter_autostart: bool,
    pub(crate) help_whitelist: Arc<RwLock<HashSet<String>>>,
    // 外部调用
    #[allow(dead_code)]
    pub(crate) tui_config: tui::TuiConfig,
    // 外部调用
    #[allow(dead_code)]
    pub(crate) locale: String,
    pub(crate) llm_runtime: LlmCommandRuntime,
    pub(crate) external_gateway: ExternalGateway,
    pub(crate) plugin_dirs: Vec<PathBuf>,
    pub(crate) disabled_commands: Vec<String>,
    pub(crate) disabled_plugins: Vec<String>,
    pub(crate) superuser_manager: SuperuserManager,
}

#[derive(Clone)]
pub(crate) struct LlmCommandRuntime {
    command_prefix: Arc<RwLock<String>>,
}

impl LlmCommandRuntime {
    pub(crate) fn new(command_prefix: impl Into<String>) -> Self {
        Self {
            command_prefix: Arc::new(RwLock::new(command_prefix.into())),
        }
    }

    pub(crate) fn command_prefix(&self) -> String {
        self.command_prefix
            .read()
            .expect("llm command prefix lock should not be poisoned")
            .clone()
    }

    // 外部调用
    #[allow(dead_code)]
    pub(crate) fn shared_command_prefix(&self) -> Arc<RwLock<String>> {
        self.command_prefix.clone()
    }

    // 外部调用
    #[allow(dead_code)]
    pub(crate) fn set_command_prefix(&self, command_prefix: impl Into<String>) {
        *self
            .command_prefix
            .write()
            .expect("llm command prefix lock should not be poisoned") = command_prefix.into();
    }
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn resolve_local_plugin_dir() -> PathBuf {
    plugin_dirs::resolve_local_plugin_dir()
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn push_explicit_plugin_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    path: &std::path::Path,
) {
    plugin_dirs::push_explicit_plugin_dir_candidates(dirs, seen, path);
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn push_runtime_plugin_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    root: &std::path::Path,
    include_dev_fallback: bool,
) {
    plugin_dirs::push_runtime_plugin_dir_candidates(dirs, seen, root, include_dev_fallback);
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn merge_llm_config_sections(
    base: Option<LlmConfigSection>,
    overlay: LlmConfigSection,
) -> LlmConfigSection {
    llm_config::merge_llm_config_sections(base, overlay)
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn resolve_password_config_path() -> PathBuf {
    bootstrap::resolve_password_config_path()
}
