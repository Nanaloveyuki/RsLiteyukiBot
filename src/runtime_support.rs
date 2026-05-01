use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use crate::app_config::{FlowLocalAgentRuntimeConfig, LlmConfigSection};
use crate::flow_local_agent::{FlowLocalAgentClient, FlowLocalAgentRuntimeState};
use crate::superuser::SuperuserManager;
use crate::tui;
use liteyukibot_core::AdapterConfig;
use liteyukibot_core::BotRuntimeConfig;
use liteyukibot_core::{LogLevel, emit_console_log};
use tokio::task::JoinHandle;

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
    pub(crate) flow_local_agent: PreparedFlowLocalAgentRuntime,
    pub(crate) external_gateway: ExternalGateway,
    pub(crate) plugin_dirs: Vec<PathBuf>,
    pub(crate) disabled_commands: Vec<String>,
    pub(crate) disabled_plugins: Vec<String>,
    pub(crate) superuser_manager: SuperuserManager,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct PreparedFlowLocalAgentRuntime {
    state: FlowLocalAgentRuntimeState,
    inner: Arc<FlowLocalAgentRuntimeInner>,
}

struct FlowLocalAgentRuntimeInner {
    config: Mutex<FlowLocalAgentRuntimeConfig>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl PreparedFlowLocalAgentRuntime {
    pub(crate) fn new(config: FlowLocalAgentRuntimeConfig) -> (Self, Vec<String>) {
        let (config, warnings) = crate::flow_local_agent::device::normalize_runtime_config(config);
        let state = FlowLocalAgentRuntimeState::default();
        (
            Self {
                state,
                inner: Arc::new(FlowLocalAgentRuntimeInner {
                    config: Mutex::new(config),
                    task: Mutex::new(None),
                }),
            },
            warnings,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn state(&self) -> FlowLocalAgentRuntimeState {
        self.state.clone()
    }

    pub(crate) fn config_snapshot(&self) -> FlowLocalAgentRuntimeConfig {
        self.inner
            .config
            .lock()
            .expect("flow local agent runtime config lock should not be poisoned")
            .clone()
    }

    pub(crate) fn spawn_background(&self) {
        let config = self.config_snapshot();
        self.restart_with_config(config);
    }

    #[allow(dead_code)]
    pub(crate) fn stop(&self, reason: impl Into<String>) {
        let reason = reason.into();
        emit_console_log(
            LogLevel::Info,
            "flow.local_agent",
            format!("flow local agent stop requested: {reason}"),
        );

        if let Some(previous) = self
            .inner
            .task
            .lock()
            .expect("flow local agent runtime task lock should not be poisoned")
            .take()
        {
            previous.abort();
        }

        self.state.mark_disconnected(false, Some(reason));
    }

    #[allow(dead_code)]
    pub(crate) fn restart_from_app_config(&self) -> Result<(), String> {
        crate::app_config::ensure_default_config_files().map_err(|err| err.to_string())?;
        let (doc, _) = crate::app_config::load_app_config_with_warnings(false);
        let config = crate::app_config::resolve_flow_local_agent_config(&doc);
        self.restart_with_config(config);
        Ok(())
    }

    pub(crate) fn restart_with_config(&self, config: FlowLocalAgentRuntimeConfig) {
        let (config, warnings) = crate::flow_local_agent::device::normalize_runtime_config(config);
        for warning in warnings {
            emit_console_log(LogLevel::Warn, "flow.local_agent", warning);
        }

        self.state.mark_disconnected(true, Some("restarting with latest config".to_string()));

        if let Some(previous) = self
            .inner
            .task
            .lock()
            .expect("flow local agent runtime task lock should not be poisoned")
            .take()
        {
            previous.abort();
        }

        *self
            .inner
            .config
            .lock()
            .expect("flow local agent runtime config lock should not be poisoned") =
            config.clone();

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent",
            format!(
                "flow local agent runtime updated (enabled={}, auto_connect={})",
                config.enabled, config.auto_connect
            ),
        );

        let client = FlowLocalAgentClient::with_runtime_config(config, self.state.clone());
        let handle = tokio::spawn(async move {
            if let Err(err) = client.run().await {
                emit_console_log(LogLevel::Warn, "flow.local_agent", format!("flow local agent stopped: {err}"));
            }
        });
        *self
            .inner
            .task
            .lock()
            .expect("flow local agent runtime task lock should not be poisoned") = Some(handle);
    }
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
