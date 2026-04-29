mod capability_service;
mod config;
mod cron_service;
mod host_async;
mod host_bridge;
mod python;
mod runtime_adapters;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::core::BotEvent;
use crate::llm::cron_task::PluginCronTaskScheduler;
use crate::observability::Logger;

use super::model::{PLUGIN_PERMISSION_ALLOW_ALL, normalize_plugin_permission};
use super::{PluginDescriptor, PluginRuntimeKind, PluginToolResult};
use config::{
    detect_config_format, read_plugin_config_document, resolve_declared_plugin_config_path,
    write_plugin_config_document,
};
pub use host_bridge::{
    PluginHostApi, PluginHostBridge, PluginSdkFuture, PluginWebApiRequest, PluginWebApiResponse,
};
pub use python::commands::{PluginScopedCommand, PluginTuiCommand};
use python::commands::{
    is_builtin_command_disabled_in_lock, is_scope_command_disabled, list_disabled_scope_commands,
    list_scope_commands, list_tui_commands, normalize_tui_command_name,
    set_builtin_command_enabled_in_lock, set_scope_command_enabled,
    sync_disabled_scope_commands_in_lock,
};
use python::execution::{execute_python_registered_tool, execute_python_registered_web_api};
use python::lifecycle::{
    dispatch_python_event, execute_python_tui_command, health_check_python_manifest_plugin,
    shutdown_python_manifest_plugin, start_python_manifest_plugin, unload_python_manifest_plugin,
};
use python::loader::load_python_manifest_plugin;
use python::state::PythonRuntimeState;
pub use runtime_adapters::{
    ExternalRuntimeAdapter, LuaRuntimeAdapter, NativeRuntimeAdapter, PluginLoadPlan,
    PluginLoadState, PythonRuntimeAdapter, RuntimeAdapter, RuntimeAdapterRegistry,
};

#[derive(Debug, Clone)]
pub enum PluginSdkError {
    UnsupportedRuntime {
        kind: PluginRuntimeKind,
        reason: String,
    },
    Host(String),
    Runtime(String),
}

impl std::fmt::Display for PluginSdkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedRuntime { kind, reason } => {
                write!(f, "unsupported runtime {:?}: {}", kind, reason)
            }
            Self::Host(reason) => write!(f, "plugin host error: {}", reason),
            Self::Runtime(reason) => write!(f, "plugin runtime error: {}", reason),
        }
    }
}

impl std::error::Error for PluginSdkError {}

#[derive(Debug, Clone, Default)]
pub(crate) struct PluginPermissionSet {
    entries: HashSet<String>,
}

impl PluginPermissionSet {
    fn from_declared(entries: &[String]) -> Result<Self, String> {
        let mut normalized = HashSet::new();
        for entry in entries {
            let Some(permission) = normalize_plugin_permission(entry.as_str()) else {
                return Err(format!("unsupported plugin permission '{}'", entry.trim()));
            };
            normalized.insert(permission);
        }

        Ok(Self {
            entries: normalized,
        })
    }

    pub(crate) fn allows(&self, permission: &str) -> bool {
        self.entries.contains(PLUGIN_PERMISSION_ALLOW_ALL) || self.entries.contains(permission)
    }
}

#[derive(Clone)]
pub struct PluginSdk {
    adapters: RuntimeAdapterRegistry,
    python_runtime: Arc<Mutex<PythonRuntimeState>>,
    cron_scheduler: Arc<Mutex<PluginCronTaskScheduler>>,
}

impl Default for PluginSdk {
    fn default() -> Self {
        Self {
            adapters: RuntimeAdapterRegistry::with_defaults(),
            python_runtime: Arc::new(Mutex::new(PythonRuntimeState::default())),
            cron_scheduler: Arc::new(Mutex::new(PluginCronTaskScheduler::from_default_path())),
        }
    }
}

impl PluginSdk {
    pub fn new(adapters: RuntimeAdapterRegistry) -> Self {
        Self {
            adapters,
            python_runtime: Arc::new(Mutex::new(PythonRuntimeState::default())),
            cron_scheduler: Arc::new(Mutex::new(PluginCronTaskScheduler::from_default_path())),
        }
    }

    pub async fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> Result<PluginLoadPlan, PluginSdkError> {
        let adapter = self.adapters.find(descriptor.runtime.kind).ok_or(
            PluginSdkError::UnsupportedRuntime {
                kind: descriptor.runtime.kind,
                reason: "no runtime adapter registered".to_string(),
            },
        )?;
        adapter.plan_load(descriptor, host).await
    }

    pub fn load_manifest_plugin(
        &self,
        descriptor: &PluginDescriptor,
        host: &PluginHostBridge,
    ) -> Result<bool, PluginSdkError> {
        match descriptor.runtime.kind {
            PluginRuntimeKind::Python => {
                let permissions =
                    PluginPermissionSet::from_declared(descriptor.permissions.as_slice())
                        .map_err(PluginSdkError::Runtime)?;
                load_python_manifest_plugin(&self.python_runtime, descriptor, host, &permissions)
            }
            _ => Ok(false),
        }
    }

    pub fn start_manifest_plugin(
        &self,
        descriptor: &PluginDescriptor,
    ) -> Result<(), PluginSdkError> {
        match descriptor.runtime.kind {
            PluginRuntimeKind::Python => {
                start_python_manifest_plugin(&self.python_runtime, descriptor.metadata.id.as_str())
            }
            _ => Ok(()),
        }
    }

    pub fn health_check_manifest_plugin(
        &self,
        descriptor: &PluginDescriptor,
    ) -> Result<(), PluginSdkError> {
        match descriptor.runtime.kind {
            PluginRuntimeKind::Python => health_check_python_manifest_plugin(
                &self.python_runtime,
                descriptor.metadata.id.as_str(),
            ),
            _ => Ok(()),
        }
    }

    pub fn shutdown_manifest_plugin(
        &self,
        descriptor: &PluginDescriptor,
    ) -> Result<(), PluginSdkError> {
        match descriptor.runtime.kind {
            PluginRuntimeKind::Python => shutdown_python_manifest_plugin(
                &self.python_runtime,
                descriptor.metadata.id.as_str(),
            ),
            _ => Ok(()),
        }
    }

    pub fn unload_manifest_plugin(
        &self,
        descriptor: &PluginDescriptor,
    ) -> Result<(), PluginSdkError> {
        match descriptor.runtime.kind {
            PluginRuntimeKind::Python => {
                unload_python_manifest_plugin(&self.python_runtime, descriptor.metadata.id.as_str())
            }
            _ => Ok(()),
        }
    }

    pub fn read_explicit_config_document(
        &self,
        descriptor: &PluginDescriptor,
    ) -> Result<Value, PluginSdkError> {
        let path = resolve_declared_plugin_config_path(descriptor)?;
        let (_, _, document) =
            read_plugin_config_document(Some(path.as_path())).map_err(PluginSdkError::Runtime)?;
        Ok(document)
    }

    pub fn write_explicit_config_document(
        &self,
        descriptor: &PluginDescriptor,
        value: &Value,
    ) -> Result<(), PluginSdkError> {
        let path = resolve_declared_plugin_config_path(descriptor)?;
        let format = detect_config_format(path.as_path());
        write_plugin_config_document(path.as_path(), format, value).map_err(PluginSdkError::Runtime)
    }

    pub fn dispatch_event(&self, event: &BotEvent, logger: &Logger) {
        dispatch_python_event(&self.python_runtime, event, logger);
    }

    pub fn is_builtin_tui_command_disabled(&self, command: &str) -> bool {
        self.is_builtin_command_disabled("tui", command)
    }

    pub fn is_builtin_command_disabled(&self, scope: &str, command: &str) -> bool {
        self.python_runtime
            .lock()
            .map(|lock| is_builtin_command_disabled_in_lock(&lock, scope, command))
            .unwrap_or(false)
    }

    pub fn set_builtin_command_enabled(
        &self,
        scope: &str,
        command: &str,
        enabled: bool,
    ) -> Result<bool, PluginSdkError> {
        let mut lock = self
            .python_runtime
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        set_builtin_command_enabled_in_lock(&mut lock, scope, command, enabled)
            .map_err(PluginSdkError::Runtime)
    }

    pub fn is_scope_command_disabled(&self, scope: &str, command: &str) -> bool {
        self.python_runtime
            .lock()
            .map(|lock| is_scope_command_disabled(&lock, scope, command))
            .unwrap_or(false)
    }

    pub fn list_tui_commands(&self) -> Vec<PluginTuiCommand> {
        list_tui_commands(&self.python_runtime)
    }

    pub fn list_scope_commands(&self, scope: &str) -> Vec<PluginScopedCommand> {
        list_scope_commands(&self.python_runtime, scope)
    }

    pub fn list_disabled_scope_commands(&self) -> Vec<String> {
        self.python_runtime
            .lock()
            .map(|lock| list_disabled_scope_commands(&lock))
            .unwrap_or_default()
    }

    pub fn sync_disabled_scope_commands(&self, entries: &[String]) -> Result<(), PluginSdkError> {
        let mut lock = self
            .python_runtime
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        sync_disabled_scope_commands_in_lock(&mut lock, entries).map_err(PluginSdkError::Runtime)
    }

    pub fn set_scope_command_enabled(
        &self,
        scope: &str,
        command: &str,
        enabled: bool,
    ) -> Result<usize, PluginSdkError> {
        set_scope_command_enabled(&self.python_runtime, scope, command, enabled)
            .map_err(PluginSdkError::Runtime)
    }

    pub fn get_tui_command(&self, command: &str) -> Option<PluginTuiCommand> {
        let command = normalize_tui_command_name(command)?;
        self.python_runtime.lock().ok().and_then(|lock| {
            lock.commands
                .get(command.as_str())
                .map(|entry| PluginTuiCommand {
                    name: entry.command.clone(),
                    description: entry.description.clone(),
                    enabled: entry.enabled
                        && !is_scope_command_disabled(&lock, "tui", entry.command.as_str()),
                    plugin_id: entry.plugin_id.clone(),
                })
        })
    }

    pub fn execute_tui_command(
        &self,
        command: &str,
        args: &[String],
    ) -> Result<Option<String>, PluginSdkError> {
        execute_python_tui_command(&self.python_runtime, command, args)
    }

    pub fn execute_plugin_web_api(
        &self,
        plugin_id: &str,
        route: &str,
        request: &PluginWebApiRequest,
    ) -> Result<Option<PluginWebApiResponse>, PluginSdkError> {
        execute_python_registered_web_api(&self.python_runtime, plugin_id, route, request)
    }

    pub fn execute_plugin_tool(
        &self,
        plugin_id: &str,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<Option<PluginToolResult>, PluginSdkError> {
        execute_python_registered_tool(&self.python_runtime, plugin_id, tool_name, arguments)
    }
}

pub(crate) fn plugin_runtime_tool_name(plugin_id: &str, tool_name: &str) -> String {
    format!("plugin::{}::{}", plugin_id.trim(), tool_name.trim())
}
