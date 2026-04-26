mod config;
mod host_async;
mod python;

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::adapter::{AdapterManager, AdapterPacket};
use crate::comm::{ChannelMessage, ChannelRegistry, SharedStore};
use crate::core::{BotEvent, LifecycleContext};
use crate::llm::cron_task::{CronTaskKey, PluginCronTaskScheduler};
use crate::llm::{LlmClientError, LlmFunctionTool, LlmToolOutput};
use crate::observability::Logger;
use crate::session::SessionRouter;

use super::abi::PluginAbiContract;
use super::model::{PLUGIN_PERMISSION_ALLOW_ALL, normalize_plugin_permission};
use super::{
    PluginCapabilitySnapshot, PluginDescriptor, PluginRegisteredCronJob, PluginRegisteredTask,
    PluginRegisteredTool, PluginRegisteredWebApi, PluginRuntimeDiagnostics, PluginRuntimeKind,
    PluginToolResult,
};
use config::{
    detect_config_format, read_plugin_config_document, resolve_declared_plugin_config_path,
    write_plugin_config_document,
};
use host_async::PluginHostAsyncExecutor;
pub use python::commands::{PluginScopedCommand, PluginTuiCommand};
use python::commands::{
    is_builtin_command_disabled_in_lock, is_scope_command_disabled, list_disabled_scope_commands,
    list_scope_commands, list_tui_commands, normalize_tui_command_name,
    set_builtin_command_enabled_in_lock, set_scope_command_enabled,
    sync_disabled_scope_commands_in_lock,
};
use python::lifecycle::{
    PythonRuntimeState, dispatch_python_event, execute_python_registered_cron_job,
    execute_python_registered_tool, execute_python_registered_web_api, execute_python_tui_command,
    get_python_plugin_capability_snapshot, get_python_plugin_runtime_diagnostics,
    health_check_python_manifest_plugin, list_all_python_plugin_capability_snapshots,
    load_python_manifest_plugin, shutdown_python_manifest_plugin, start_python_manifest_plugin,
    unload_python_manifest_plugin,
};
use python::probe::probe_python_plugin_compatibility;

const HOST_PLUGIN_API_VERSION: &str = "0.1";

pub type PluginSdkFuture<T> = Pin<Box<dyn Future<Output = Result<T, PluginSdkError>> + Send>>;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginLoadState {
    Ready,
    Deferred,
}

#[derive(Debug, Clone, Default)]
pub struct PluginWebApiRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub peer_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginWebApiResponse {
    pub status_code: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PluginLoadPlan {
    pub runtime_kind: PluginRuntimeKind,
    pub state: PluginLoadState,
    pub reason: Option<String>,
    pub contract: PluginAbiContract,
}

impl PluginLoadPlan {
    pub fn ready(runtime_kind: PluginRuntimeKind, contract: PluginAbiContract) -> Self {
        Self {
            runtime_kind,
            state: PluginLoadState::Ready,
            reason: None,
            contract,
        }
    }

    pub fn deferred(
        runtime_kind: PluginRuntimeKind,
        contract: PluginAbiContract,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            runtime_kind,
            state: PluginLoadState::Deferred,
            reason: Some(reason.into()),
            contract,
        }
    }
}

pub trait PluginHostApi: Send + Sync {
    fn log(&self, message: String) -> PluginSdkFuture<()>;
    fn publish(&self, channel_name: String, topic: String, payload: Value) -> PluginSdkFuture<()>;
    fn kv_get(&self, key: String) -> PluginSdkFuture<Option<Value>>;
    fn kv_set(&self, key: String, value: Value) -> PluginSdkFuture<()>;
    fn host_app_version(&self) -> &str;
    fn host_api_version(&self) -> &'static str;
}

#[derive(Clone)]
pub struct PluginHostBridge {
    lifecycle: Arc<LifecycleContext>,
    channels: ChannelRegistry,
    shared_store: SharedStore,
    session_router: SessionRouter,
    adapter_manager: AdapterManager,
    logger: Logger,
    async_executor: PluginHostAsyncExecutor,
}

impl PluginHostBridge {
    pub fn new(
        lifecycle: Arc<LifecycleContext>,
        channels: ChannelRegistry,
        shared_store: SharedStore,
        session_router: SessionRouter,
        adapter_manager: AdapterManager,
        logger: Logger,
    ) -> Self {
        Self {
            lifecycle,
            channels,
            shared_store,
            session_router,
            adapter_manager,
            logger,
            async_executor: PluginHostAsyncExecutor::new(),
        }
    }

    pub fn lifecycle(&self) -> Arc<LifecycleContext> {
        self.lifecycle.clone()
    }

    pub fn channels(&self) -> &ChannelRegistry {
        &self.channels
    }

    pub fn shared_store(&self) -> &SharedStore {
        &self.shared_store
    }

    pub fn session_router(&self) -> &SessionRouter {
        &self.session_router
    }

    pub fn adapter_manager(&self) -> &AdapterManager {
        &self.adapter_manager
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn reply_onebot_text(
        &self,
        event: &Value,
        message: &str,
        plugin_id: &str,
    ) -> Result<bool, String> {
        let text = message.trim();
        if text.is_empty() {
            return Ok(false);
        }
        let payload = event
            .get("payload")
            .and_then(Value::as_object)
            .ok_or_else(|| "event payload is missing for onebot reply".to_string())?;
        let adapter_id = payload
            .get("_adapter_id")
            .and_then(value_to_string)
            .ok_or_else(|| "event payload missing _adapter_id".to_string())?;

        let send_payload = build_onebot_v11_text_reply_payload(payload, text)
            .ok_or_else(|| "event payload is not a supported onebot message event".to_string())?;
        let packet_id = format!("plugin-{}-{}", plugin_id, now_millis());
        let adapter_manager = self.adapter_manager.clone();
        let logger = self.logger.clone();
        let plugin_id = plugin_id.to_string();
        let packet = AdapterPacket::new(packet_id, "onebot.v11.api.send_msg", send_payload);

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let adapter_manager = adapter_manager.clone();
            let logger = logger.clone();
            let plugin_id = plugin_id.clone();
            let adapter_id = adapter_id.clone();
            let packet = packet.clone();
            handle.spawn(async move {
                if let Err(err) = adapter_manager.send(&adapter_id, packet).await {
                    logger.warn_in(
                        "plugin.python",
                        format!(
                            "plugin '{}' onebot reply send failed (adapter={}): {}",
                            plugin_id, adapter_id, err
                        ),
                    );
                }
            });
        } else {
            self.async_executor.dispatch_onebot_reply(
                adapter_manager,
                adapter_id,
                packet,
                logger,
                plugin_id,
            )?;
        }
        Ok(true)
    }
}

impl PluginHostApi for PluginHostBridge {
    fn log(&self, message: String) -> PluginSdkFuture<()> {
        let logger = self.logger.clone();
        Box::pin(async move {
            logger.info_in("plugin.host", message);
            Ok(())
        })
    }

    fn publish(&self, channel_name: String, topic: String, payload: Value) -> PluginSdkFuture<()> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move {
            let message = ChannelMessage::new(topic, payload, Some("plugin-sdk"));
            shared_store
                .publish(&channel_name, message)
                .map_err(|err| PluginSdkError::Host(err.to_string()))?;
            Ok(())
        })
    }

    fn kv_get(&self, key: String) -> PluginSdkFuture<Option<Value>> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move { Ok(shared_store.get(&key)) })
    }

    fn kv_set(&self, key: String, value: Value) -> PluginSdkFuture<()> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move {
            shared_store.set(key, value);
            Ok(())
        })
    }

    fn host_app_version(&self) -> &str {
        self.lifecycle.app_version()
    }

    fn host_api_version(&self) -> &'static str {
        HOST_PLUGIN_API_VERSION
    }
}

pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> PluginRuntimeKind;
    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan>;
}

#[derive(Clone, Default)]
pub struct RuntimeAdapterRegistry {
    adapters: Vec<Arc<dyn RuntimeAdapter>>,
}

impl RuntimeAdapterRegistry {
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        registry.register(NativeRuntimeAdapter);
        registry.register(PythonRuntimeAdapter);
        registry.register(LuaRuntimeAdapter);
        registry.register(ExternalRuntimeAdapter);
        registry
    }

    pub fn register<A: RuntimeAdapter + 'static>(&mut self, adapter: A) {
        self.adapters.push(Arc::new(adapter));
    }

    pub fn find(&self, kind: PluginRuntimeKind) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.kind() == kind)
            .cloned()
    }
}

pub struct ExternalRuntimeAdapter;

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

    pub fn get_plugin_capabilities(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginCapabilitySnapshot>, PluginSdkError> {
        let mut snapshot = self.get_plugin_capabilities_raw(plugin_id)?;
        if let Some(snapshot) = snapshot.as_mut() {
            self.sync_plugin_cron_snapshot(snapshot, Utc::now())?;
        }
        Ok(snapshot)
    }

    pub fn list_plugin_tools(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginRegisteredTool>, PluginSdkError> {
        Ok(self
            .get_plugin_capabilities(plugin_id)?
            .map(|snapshot| snapshot.tools)
            .unwrap_or_default())
    }

    pub fn list_plugin_web_apis(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginRegisteredWebApi>, PluginSdkError> {
        Ok(self
            .get_plugin_capabilities(plugin_id)?
            .map(|snapshot| snapshot.web_apis)
            .unwrap_or_default())
    }

    pub fn list_plugin_cron_jobs(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginRegisteredCronJob>, PluginSdkError> {
        Ok(self
            .get_plugin_capabilities(plugin_id)?
            .map(|snapshot| snapshot.cron_jobs)
            .unwrap_or_default())
    }

    pub fn list_plugin_tasks(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginRegisteredTask>, PluginSdkError> {
        Ok(self
            .get_plugin_capabilities(plugin_id)?
            .map(|snapshot| snapshot.tasks)
            .unwrap_or_default())
    }

    pub fn list_all_plugin_capabilities(
        &self,
    ) -> Result<Vec<PluginCapabilitySnapshot>, PluginSdkError> {
        let mut snapshots = self.list_all_plugin_capabilities_raw()?;
        self.sync_all_plugin_cron_snapshots(snapshots.as_mut_slice(), true, Utc::now())?;
        Ok(snapshots)
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

    pub fn build_plugin_tool_bundle(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<LlmFunctionTool>, PluginSdkError> {
        let tools = self.list_plugin_tools(plugin_id)?;
        let mut bundle = Vec::new();
        for tool in tools.into_iter().filter(|tool| tool.active) {
            let runtime_name = plugin_runtime_tool_name(plugin_id, tool.name.as_str());
            let original_name = tool.name.clone();
            let description = tool.description.clone();
            let parameters = tool.parameters.clone();
            let sdk = self.clone();
            let plugin_id = plugin_id.to_string();
            let runtime_name_for_handler = runtime_name.clone();
            let original_name_for_handler = original_name.clone();
            let llm_tool = LlmFunctionTool::new(runtime_name, parameters, move |arguments| {
                let sdk = sdk.clone();
                let plugin_id = plugin_id.clone();
                let runtime_name = runtime_name_for_handler.clone();
                let original_name = original_name_for_handler.clone();
                async move {
                    match sdk.execute_plugin_tool(&plugin_id, &original_name, &arguments) {
                        Ok(Some(PluginToolResult::Text(text))) => Ok(LlmToolOutput::Text(text)),
                        Ok(Some(PluginToolResult::Json(value))) => Ok(LlmToolOutput::Json(value)),
                        Ok(None) => Err(LlmClientError::Tool(format!(
                            "plugin tool '{}' is unavailable",
                            runtime_name
                        ))),
                        Err(err) => Err(LlmClientError::Tool(err.to_string())),
                    }
                }
            })
            .with_description(description);
            bundle.push(llm_tool);
        }
        Ok(bundle)
    }

    pub fn build_all_plugin_tool_bundle(&self) -> Result<Vec<LlmFunctionTool>, PluginSdkError> {
        let snapshots = self.list_all_plugin_capabilities()?;
        let mut bundle = Vec::new();
        let mut seen_names = HashSet::new();
        for snapshot in snapshots {
            for tool in self.build_plugin_tool_bundle(snapshot.plugin_id.as_str())? {
                if !seen_names.insert(tool.name.clone()) {
                    return Err(PluginSdkError::Runtime(format!(
                        "duplicate plugin runtime tool name '{}'",
                        tool.name
                    )));
                }
                bundle.push(tool);
            }
        }
        Ok(bundle)
    }

    pub fn get_plugin_runtime_diagnostics(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginRuntimeDiagnostics>, PluginSdkError> {
        get_python_plugin_runtime_diagnostics(&self.python_runtime, plugin_id)
    }

    pub fn run_due_plugin_jobs(
        &self,
        disabled_plugin_ids: &[String],
        now: Option<DateTime<Utc>>,
    ) -> Result<usize, PluginSdkError> {
        let now = now.unwrap_or_else(Utc::now);
        let mut snapshots = self.list_all_plugin_capabilities_raw()?;
        self.sync_all_plugin_cron_snapshots(snapshots.as_mut_slice(), true, now)?;
        let disabled = disabled_plugin_ids.iter().cloned().collect::<HashSet<_>>();
        let due_jobs = {
            let scheduler = self.cron_scheduler.lock().map_err(|_| {
                PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
            })?;
            scheduler.collect_due_jobs(snapshots.as_slice(), &disabled, now)
        };

        if due_jobs.is_empty() {
            return Ok(0);
        }

        let job_map = snapshots
            .iter()
            .flat_map(|snapshot| {
                snapshot.cron_jobs.iter().cloned().map(|job| {
                    (
                        CronTaskKey::new(snapshot.plugin_id.clone(), job.job_id.clone()),
                        job,
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();

        let mut executed = 0usize;
        for due_job in due_jobs {
            let Some(job) = job_map.get(&due_job.key) else {
                continue;
            };
            match execute_python_registered_cron_job(
                &self.python_runtime,
                due_job.key.plugin_id.as_str(),
                due_job.key.job_id.as_str(),
                &due_job.job.payload,
            ) {
                Ok(true) => {
                    executed = executed.saturating_add(1);
                    self.cron_scheduler
                        .lock()
                        .map_err(|_| {
                            PluginSdkError::Runtime(
                                "plugin cron scheduler lock poisoned".to_string(),
                            )
                        })?
                        .mark_job_success(&due_job.key, job, now)
                        .map_err(PluginSdkError::Runtime)?;
                }
                Ok(false) => {
                    self.cron_scheduler
                        .lock()
                        .map_err(|_| {
                            PluginSdkError::Runtime(
                                "plugin cron scheduler lock poisoned".to_string(),
                            )
                        })?
                        .mark_job_error(
                            &due_job.key,
                            job,
                            now,
                            format!(
                                "plugin cron job '{}' does not expose an executable handler",
                                due_job.key.job_id
                            ),
                        )
                        .map_err(PluginSdkError::Runtime)?;
                }
                Err(err) => {
                    self.cron_scheduler
                        .lock()
                        .map_err(|_| {
                            PluginSdkError::Runtime(
                                "plugin cron scheduler lock poisoned".to_string(),
                            )
                        })?
                        .mark_job_error(&due_job.key, job, now, err.to_string())
                        .map_err(PluginSdkError::Runtime)?;
                }
            }
        }

        Ok(executed)
    }

    pub fn plugin_cron_scheduler_status(&self, plugin_id: &str) -> Result<String, PluginSdkError> {
        let Some(mut snapshot) = self.get_plugin_capabilities_raw(plugin_id)? else {
            return Ok("unsupported".to_string());
        };
        self.sync_plugin_cron_snapshot(&mut snapshot, Utc::now())?;
        let scheduler = self.cron_scheduler.lock().map_err(|_| {
            PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
        })?;
        Ok(scheduler.plugin_scheduler_status(&snapshot))
    }

    pub fn plugin_has_executable_cron_jobs(&self, plugin_id: &str) -> Result<bool, PluginSdkError> {
        let Some(mut snapshot) = self.get_plugin_capabilities_raw(plugin_id)? else {
            return Ok(false);
        };
        self.sync_plugin_cron_snapshot(&mut snapshot, Utc::now())?;
        let scheduler = self.cron_scheduler.lock().map_err(|_| {
            PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
        })?;
        Ok(scheduler.plugin_has_executable_jobs(&snapshot))
    }

    fn get_plugin_capabilities_raw(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginCapabilitySnapshot>, PluginSdkError> {
        get_python_plugin_capability_snapshot(&self.python_runtime, plugin_id)
    }

    fn list_all_plugin_capabilities_raw(
        &self,
    ) -> Result<Vec<PluginCapabilitySnapshot>, PluginSdkError> {
        list_all_python_plugin_capability_snapshots(&self.python_runtime)
    }

    fn sync_plugin_cron_snapshot(
        &self,
        snapshot: &mut PluginCapabilitySnapshot,
        now: DateTime<Utc>,
    ) -> Result<(), PluginSdkError> {
        self.cron_scheduler
            .lock()
            .map_err(|_| {
                PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
            })?
            .sync_snapshot(snapshot, now)
            .map_err(PluginSdkError::Runtime)
    }

    fn sync_all_plugin_cron_snapshots(
        &self,
        snapshots: &mut [PluginCapabilitySnapshot],
        prune_missing: bool,
        now: DateTime<Utc>,
    ) -> Result<(), PluginSdkError> {
        self.cron_scheduler
            .lock()
            .map_err(|_| {
                PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
            })?
            .sync_snapshots(snapshots, prune_missing, now)
            .map_err(PluginSdkError::Runtime)
    }
}

pub(crate) fn plugin_runtime_tool_name(plugin_id: &str, tool_name: &str) -> String {
    format!("plugin::{}::{}", plugin_id.trim(), tool_name.trim())
}

pub struct NativeRuntimeAdapter;
pub struct PythonRuntimeAdapter;
pub struct LuaRuntimeAdapter;

impl RuntimeAdapter for NativeRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Native
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = build_plugin_contract(
            descriptor,
            PluginRuntimeKind::Native,
            "liteyuki-native",
            host,
            true,
        );
        let has_entry = !descriptor.runtime.entrypoint.trim().is_empty()
            || !descriptor.runtime.module.trim().is_empty();
        Box::pin(async move {
            let contract = contract?;
            if has_entry {
                Ok(PluginLoadPlan::ready(PluginRuntimeKind::Native, contract))
            } else {
                Ok(PluginLoadPlan::deferred(
                    PluginRuntimeKind::Native,
                    contract,
                    "native plugin entrypoint is not declared",
                ))
            }
        })
    }
}

impl RuntimeAdapter for PythonRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Python
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = build_plugin_contract(
            descriptor,
            PluginRuntimeKind::Python,
            "liteyuki-python-bridge",
            host,
            true,
        );
        let probe = probe_python_plugin_compatibility(descriptor);
        Box::pin(async move {
            let contract = contract?;
            match probe {
                Ok(_) => Ok(PluginLoadPlan::ready(PluginRuntimeKind::Python, contract)),
                Err(reason) => Ok(PluginLoadPlan::deferred(
                    PluginRuntimeKind::Python,
                    contract,
                    reason,
                )),
            }
        })
    }
}

impl RuntimeAdapter for LuaRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Lua
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = build_plugin_contract(
            descriptor,
            PluginRuntimeKind::Lua,
            "liteyuki-lua-bridge",
            host,
            false,
        );
        Box::pin(async move {
            let contract = contract?;
            Ok(PluginLoadPlan::deferred(
                PluginRuntimeKind::Lua,
                contract,
                "lua runtime bridge is reserved for future lua integration",
            ))
        })
    }
}

impl RuntimeAdapter for ExternalRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::External
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = build_plugin_contract(
            descriptor,
            PluginRuntimeKind::External,
            "liteyuki-external-bridge",
            host,
            false,
        );
        Box::pin(async move {
            let contract = contract?;
            Ok(PluginLoadPlan::deferred(
                PluginRuntimeKind::External,
                contract,
                "external runtime bridge is reserved for managed sidecar integration",
            ))
        })
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    if let Some(raw) = value.as_str() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_u64() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_i64() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_bool() {
        return Some(raw.to_string());
    }
    None
}

fn build_onebot_v11_text_reply_payload(
    event_payload: &serde_json::Map<String, Value>,
    text: &str,
) -> Option<Value> {
    let message_type = event_payload
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("private")
        .to_ascii_lowercase();
    let echo = format!("plugin-reply-{}", now_millis());
    let mut params = serde_json::Map::new();
    params.insert(
        "message_type".to_string(),
        Value::String(message_type.clone()),
    );
    params.insert("message".to_string(), Value::String(text.to_string()));
    params.insert("auto_escape".to_string(), Value::Bool(false));

    match message_type.as_str() {
        "group" => {
            params.insert(
                "group_id".to_string(),
                event_payload.get("group_id")?.clone(),
            );
        }
        _ => {
            params.insert("user_id".to_string(), event_payload.get("user_id")?.clone());
            params.insert(
                "message_type".to_string(),
                Value::String("private".to_string()),
            );
        }
    }
    Some(Value::Object(serde_json::Map::from_iter([
        ("action".to_string(), Value::String("send_msg".to_string())),
        ("params".to_string(), Value::Object(params)),
        ("echo".to_string(), Value::String(echo)),
    ])))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn normalize_abi_version(raw: &str) -> String {
    if raw.trim().is_empty() {
        "1.0".to_string()
    } else {
        raw.trim().to_string()
    }
}

fn build_plugin_contract(
    descriptor: &PluginDescriptor,
    runtime_kind: PluginRuntimeKind,
    abi_name: &str,
    host: &dyn PluginHostApi,
    requires_handle_event: bool,
) -> Result<PluginAbiContract, PluginSdkError> {
    validate_declared_permissions(descriptor.permissions.as_slice(), runtime_kind)?;

    let host_api_version = normalize_host_api_version(host.host_api_version());
    let requested_api_version = parse_version_components(
        descriptor.sdk.api_version.as_str(),
        Some(HOST_PLUGIN_API_VERSION),
        runtime_kind,
        "sdk.api_version",
    )?;
    let host_api_components = parse_version_components(
        host_api_version.as_str(),
        Some(HOST_PLUGIN_API_VERSION),
        runtime_kind,
        "host api version",
    )?;
    let requested_display = format_version_components(requested_api_version.as_slice());
    let host_api_display = format_version_components(host_api_components.as_slice());
    if requested_api_version.first().copied().unwrap_or_default()
        != host_api_components.first().copied().unwrap_or_default()
        || compare_version_components(
            host_api_components.as_slice(),
            requested_api_version.as_slice(),
        ) == Ordering::Less
    {
        return Err(PluginSdkError::UnsupportedRuntime {
            kind: runtime_kind,
            reason: format!(
                "plugin SDK api_version '{}' is not supported by host api {}",
                requested_display, host_api_display
            ),
        });
    }

    if !descriptor.sdk.min_host_version.trim().is_empty() {
        let minimum_host = parse_version_components(
            descriptor.sdk.min_host_version.as_str(),
            None,
            runtime_kind,
            "sdk.min_host_version",
        )?;
        let actual_host = parse_version_components(
            host.host_app_version(),
            None,
            runtime_kind,
            "host app version",
        )?;
        if compare_version_components(actual_host.as_slice(), minimum_host.as_slice())
            == Ordering::Less
        {
            return Err(PluginSdkError::UnsupportedRuntime {
                kind: runtime_kind,
                reason: format!(
                    "plugin requires host version >= {} but current host is {}",
                    format_version_components(minimum_host.as_slice()),
                    format_version_components(actual_host.as_slice())
                ),
            });
        }
    }

    let mut contract = PluginAbiContract::new(
        runtime_kind,
        abi_name,
        normalize_abi_version(&descriptor.runtime.abi),
        host_api_version,
    );
    if requires_handle_event {
        contract
            .required_methods
            .push(super::abi::PluginAbiMethod::HandleEvent);
    }
    Ok(contract)
}

fn validate_declared_permissions(
    permissions: &[String],
    runtime_kind: PluginRuntimeKind,
) -> Result<(), PluginSdkError> {
    PluginPermissionSet::from_declared(permissions)
        .map(|_| ())
        .map_err(|err| PluginSdkError::UnsupportedRuntime {
            kind: runtime_kind,
            reason: err,
        })
}

fn normalize_host_api_version(raw: &str) -> String {
    if raw.trim().is_empty() {
        HOST_PLUGIN_API_VERSION.to_string()
    } else {
        raw.trim().to_string()
    }
}

fn parse_version_components(
    raw: &str,
    default_value: Option<&str>,
    runtime_kind: PluginRuntimeKind,
    field_name: &str,
) -> Result<Vec<u64>, PluginSdkError> {
    let candidate = if raw.trim().is_empty() {
        default_value.unwrap_or("")
    } else {
        raw.trim()
    };
    let candidate = candidate
        .split(['-', '+'])
        .next()
        .unwrap_or(candidate)
        .trim();
    if candidate.is_empty() {
        return Err(PluginSdkError::UnsupportedRuntime {
            kind: runtime_kind,
            reason: format!("{field_name} should not be empty"),
        });
    }

    let mut components = Vec::new();
    for segment in candidate.split('.') {
        if segment.is_empty() || !segment.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(PluginSdkError::UnsupportedRuntime {
                kind: runtime_kind,
                reason: format!("{field_name} should use dot-separated numeric versions"),
            });
        }
        let value = segment
            .parse::<u64>()
            .map_err(|_| PluginSdkError::UnsupportedRuntime {
                kind: runtime_kind,
                reason: format!("{field_name} contains an out-of-range version segment"),
            })?;
        components.push(value);
    }

    while components.len() > 1 && components.last() == Some(&0) {
        components.pop();
    }
    Ok(components)
}

fn compare_version_components(left: &[u64], right: &[u64]) -> Ordering {
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let lhs = left.get(index).copied().unwrap_or(0);
        let rhs = right.get(index).copied().unwrap_or(0);
        match lhs.cmp(&rhs) {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }
    Ordering::Equal
}

fn format_version_components(components: &[u64]) -> String {
    components
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn onebot_reply_payload_uses_action_envelope_for_group() {
        let payload = json!({
            "message_type": "group",
            "group_id": 112233
        });
        let built = build_onebot_v11_text_reply_payload(
            payload.as_object().expect("test payload should be object"),
            "hello",
        )
        .expect("group payload should build");

        assert_eq!(
            built.get("action").and_then(Value::as_str),
            Some("send_msg")
        );
        assert_eq!(
            built.get("params").and_then(|v| v.get("group_id")),
            Some(&json!(112233))
        );
        assert_eq!(
            built.get("params").and_then(|v| v.get("message")),
            Some(&json!("hello"))
        );
    }

    #[test]
    fn onebot_reply_payload_uses_private_target_for_direct_message() {
        let payload = json!({
            "message_type": "private",
            "user_id": "445566"
        });
        let built = build_onebot_v11_text_reply_payload(
            payload.as_object().expect("test payload should be object"),
            "pong",
        )
        .expect("private payload should build");

        assert_eq!(
            built.get("params").and_then(|v| v.get("message_type")),
            Some(&json!("private"))
        );
        assert_eq!(
            built.get("params").and_then(|v| v.get("user_id")),
            Some(&json!("445566"))
        );
    }
}
