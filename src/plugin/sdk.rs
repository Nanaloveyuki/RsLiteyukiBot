use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyModule, PyTuple};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::adapter::{AdapterManager, AdapterPacket};
use crate::comm::{ChannelMessage, ChannelRegistry, SharedStore};
use crate::core::{BotEvent, LifecycleContext};
use crate::observability::Logger;
use crate::session::SessionRouter;

use super::abi::PluginAbiContract;
use super::model::{PLUGIN_PERMISSION_ALLOW_ALL, normalize_plugin_permission};
use super::{PluginCommandDescriptor, PluginDescriptor, PluginRuntimeKind};

const PYTHON_META_ATTRS: [&str; 3] = [
    "__plugin_meta__",
    "__plugin_metadata__",
    "__liteyuki_plugin_meta__",
];
const PYTHON_EVENT_HANDLER_ATTRS: [&str; 3] = ["on_event", "handle_event", "liteyuki_handle_event"];
const PYTHON_START_HANDLER_ATTRS: [&str; 3] = ["on_start", "start", "liteyuki_start"];
const PYTHON_HEALTH_HANDLER_ATTRS: [&str; 3] =
    ["on_health_check", "health_check", "liteyuki_health_check"];
const PYTHON_UNLOAD_HANDLER_ATTRS: [&str; 3] = ["on_unload", "unload", "liteyuki_unload"];
const PYTHON_SHUTDOWN_HANDLER_ATTRS: [&str; 3] = ["on_shutdown", "shutdown", "liteyuki_shutdown"];
const DEFAULT_PLUGIN_CONFIG_PATHS: [&str; 6] = [
    "config.yaml",
    "rust-config.yaml",
    "rust-config.yml",
    "rust-config.toml",
    "config/rust-core.yaml",
    "config/rust-core.toml",
];
static PLUGIN_CONFIG_RW_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
const HOST_PLUGIN_API_VERSION: &str = "0.1";
const PERMISSION_KV_READ: &str = "kv.read";
const PERMISSION_KV_WRITE: &str = "kv.write";
const PERMISSION_CHANNEL_PUBLISH: &str = "channel.publish";
const PERMISSION_ADAPTER_REPLY: &str = "adapter.reply";
const PERMISSION_CONFIG_READ: &str = "config.read";
const PERMISSION_CONFIG_WRITE: &str = "config.write";
const PERMISSION_COMMAND_TUI_READ: &str = "command.tui.read";
const PERMISSION_COMMAND_TUI_MANAGE: &str = "command.tui.manage";

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
        tokio::spawn(async move {
            let packet = AdapterPacket::new(packet_id, "onebot.v11.api.send_msg", send_payload);
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginTuiCommand {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginScopedCommand {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub plugin_id: String,
    pub scopes: Vec<String>,
    pub executable_in_tui: bool,
}

#[derive(Default)]
struct PythonRuntimeState {
    plugins: HashMap<String, PythonLoadedPlugin>,
    commands: HashMap<String, PythonTuiCommandEntry>,
    declared_commands: Vec<PythonDeclaredCommandEntry>,
    disabled_scope_commands: HashSet<ScopedCommandKey>,
}

struct PythonLoadedPlugin {
    event_handler: Option<Py<PyAny>>,
    start_handler: Option<Py<PyAny>>,
    health_handler: Option<Py<PyAny>>,
    shutdown_handler: Option<Py<PyAny>>,
    unload_handler: Option<Py<PyAny>>,
    sdk: Py<PyPluginSdk>,
}

struct PythonTuiCommandEntry {
    command: String,
    description: String,
    enabled: bool,
    plugin_id: String,
    handler: Py<PyAny>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ScopedCommandKey {
    scope: String,
    command: String,
}

#[derive(Debug, Clone)]
struct PythonDeclaredCommandEntry {
    command: String,
    description: String,
    plugin_id: String,
    scopes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct PluginPermissionSet {
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

    fn allows(&self, permission: &str) -> bool {
        self.entries.contains(PLUGIN_PERMISSION_ALLOW_ALL) || self.entries.contains(permission)
    }
}

#[pyclass]
#[derive(Clone)]
struct PyPluginSdk {
    plugin_id: String,
    host: PluginHostBridge,
    runtime_state: Arc<Mutex<PythonRuntimeState>>,
    config_path: Option<PathBuf>,
    permissions: PluginPermissionSet,
}

#[pymethods]
impl PyPluginSdk {
    #[getter]
    fn plugin_id(&self) -> String {
        self.plugin_id.clone()
    }

    fn log(&self, message: String) {
        self.host.logger().info_in(
            "plugin.python",
            format!("[{}] {}", self.plugin_id, message.trim()),
        );
    }

    fn info(&self, message: String) {
        self.log(message);
    }

    fn warning(&self, message: String) {
        self.host.logger().warn_in(
            "plugin.python",
            format!("[{}] {}", self.plugin_id, message.trim()),
        );
    }

    fn warn(&self, message: String) {
        self.warning(message);
    }

    fn error(&self, message: String) {
        self.host.logger().error_in(
            "plugin.python",
            format!("[{}] {}", self.plugin_id, message.trim()),
        );
    }

    fn success(&self, message: String) {
        self.host.logger().info_in(
            "plugin.python",
            format!("[{}] {}", self.plugin_id, message.trim()),
        );
    }

    fn kv_get(&self, py: Python<'_>, key: String) -> PyResult<PyObject> {
        self.ensure_permission(PERMISSION_KV_READ, "read shared kv values")?;
        let value = self
            .host
            .shared_store()
            .get(key.trim())
            .unwrap_or(Value::Null);
        json_to_pyobject(py, &value)
    }

    fn kv_set(&self, py: Python<'_>, key: String, value: Py<PyAny>) -> PyResult<()> {
        self.ensure_permission(PERMISSION_KV_WRITE, "write shared kv values")?;
        let key = key.trim();
        if key.is_empty() {
            return Err(PyValueError::new_err("kv key should not be empty"));
        }
        let value = py_any_to_json(value.bind(py))?;
        self.host.shared_store().set(key.to_string(), value);
        Ok(())
    }

    fn kv_delete(&self, key: String) -> bool {
        if self
            .ensure_permission(PERMISSION_KV_WRITE, "delete shared kv values")
            .is_err()
        {
            return false;
        }
        let key = key.trim();
        if key.is_empty() {
            return false;
        }
        self.host.shared_store().delete(key).is_some()
    }

    fn publish(
        &self,
        py: Python<'_>,
        channel_name: String,
        topic: String,
        payload: Py<PyAny>,
    ) -> PyResult<()> {
        self.ensure_permission(PERMISSION_CHANNEL_PUBLISH, "publish channel messages")?;
        let channel_name = channel_name.trim();
        let topic = topic.trim();
        if channel_name.is_empty() {
            return Err(PyValueError::new_err("channel_name should not be empty"));
        }
        if topic.is_empty() {
            return Err(PyValueError::new_err("topic should not be empty"));
        }
        let payload = py_any_to_json(payload.bind(py))?;
        let message = ChannelMessage::new(topic, payload, Some("python-plugin"));
        self.host
            .shared_store()
            .publish(channel_name, message)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))
    }

    fn reply_text(&self, py: Python<'_>, event: Py<PyAny>, message: String) -> PyResult<bool> {
        self.ensure_permission(PERMISSION_ADAPTER_REPLY, "reply through adapters")?;
        let event = py_any_to_json(event.bind(py))?;
        self.host
            .reply_onebot_text(&event, message.as_str(), self.plugin_id.as_str())
            .map_err(PyRuntimeError::new_err)
    }

    fn config_get(&self, py: Python<'_>, key: String) -> PyResult<PyObject> {
        self.ensure_permission(PERMISSION_CONFIG_READ, "read plugin config")?;
        let value = read_config_value(self.config_path.as_deref(), key.as_str())
            .map_err(PyRuntimeError::new_err)?
            .unwrap_or(Value::Null);
        json_to_pyobject(py, &value)
    }

    fn config_set(&self, py: Python<'_>, key: String, value: Py<PyAny>) -> PyResult<()> {
        self.ensure_permission(PERMISSION_CONFIG_WRITE, "write plugin config")?;
        let value = py_any_to_json(value.bind(py))?;
        write_config_value(self.config_path.as_deref(), key.as_str(), value)
            .map_err(PyRuntimeError::new_err)
    }

    fn config_delete(&self, key: String) -> PyResult<bool> {
        self.ensure_permission(PERMISSION_CONFIG_WRITE, "write plugin config")?;
        delete_config_value(self.config_path.as_deref(), key.as_str())
            .map_err(PyRuntimeError::new_err)
    }

    #[pyo3(signature = (command, handler, description=None, enabled=None))]
    fn add_tui_command(
        &self,
        py: Python<'_>,
        command: String,
        handler: Py<PyAny>,
        description: Option<String>,
        enabled: Option<bool>,
    ) -> PyResult<()> {
        self.ensure_permission(PERMISSION_COMMAND_TUI_MANAGE, "manage TUI commands")?;
        register_tui_command(
            &self.runtime_state,
            self.plugin_id.as_str(),
            command.as_str(),
            description,
            enabled.unwrap_or(true),
            handler,
            py,
        )
        .map_err(PyRuntimeError::new_err)
    }

    fn disable_tui_command(&self, command: String) -> PyResult<bool> {
        self.ensure_permission(PERMISSION_COMMAND_TUI_MANAGE, "manage TUI commands")?;
        set_tui_command_enabled(
            &self.runtime_state,
            self.plugin_id.as_str(),
            command.as_str(),
            false,
        )
        .map_err(PyRuntimeError::new_err)
    }

    fn enable_tui_command(&self, command: String) -> PyResult<bool> {
        self.ensure_permission(PERMISSION_COMMAND_TUI_MANAGE, "manage TUI commands")?;
        set_tui_command_enabled(
            &self.runtime_state,
            self.plugin_id.as_str(),
            command.as_str(),
            true,
        )
        .map_err(PyRuntimeError::new_err)
    }

    fn remove_tui_command(&self, command: String) -> PyResult<bool> {
        self.ensure_permission(PERMISSION_COMMAND_TUI_MANAGE, "manage TUI commands")?;
        remove_tui_command(
            &self.runtime_state,
            self.plugin_id.as_str(),
            command.as_str(),
        )
        .map_err(PyRuntimeError::new_err)
    }

    fn list_tui_commands(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.ensure_permission(PERMISSION_COMMAND_TUI_READ, "read TUI command catalog")?;
        let commands = list_tui_commands(&self.runtime_state);
        let value = serde_json::to_value(commands)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        json_to_pyobject(py, &value)
    }
}

impl PyPluginSdk {
    fn new(
        plugin_id: String,
        host: PluginHostBridge,
        runtime_state: Arc<Mutex<PythonRuntimeState>>,
        config_path: Option<PathBuf>,
        permissions: PluginPermissionSet,
    ) -> Self {
        Self {
            plugin_id,
            host,
            runtime_state,
            config_path,
            permissions,
        }
    }

    fn ensure_permission(&self, permission: &'static str, action: &str) -> PyResult<()> {
        if self.permissions.allows(permission) {
            return Ok(());
        }
        Err(PyRuntimeError::new_err(format!(
            "plugin '{}' is not allowed to {} (missing permission '{}')",
            self.plugin_id, action, permission
        )))
    }
}

#[derive(Clone)]
pub struct PluginSdk {
    adapters: RuntimeAdapterRegistry,
    python_runtime: Arc<Mutex<PythonRuntimeState>>,
}

impl Default for PluginSdk {
    fn default() -> Self {
        Self {
            adapters: RuntimeAdapterRegistry::with_defaults(),
            python_runtime: Arc::new(Mutex::new(PythonRuntimeState::default())),
        }
    }
}

impl PluginSdk {
    pub fn new(adapters: RuntimeAdapterRegistry) -> Self {
        Self {
            adapters,
            python_runtime: Arc::new(Mutex::new(PythonRuntimeState::default())),
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
            PluginRuntimeKind::Python => self.load_python_manifest_plugin(descriptor, host),
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

    pub fn dispatch_event(&self, event: &BotEvent, logger: &Logger) {
        let handlers: Vec<(String, Py<PyAny>, Py<PyPluginSdk>)> = match Python::with_gil(
            |py| -> Result<Vec<(String, Py<PyAny>, Py<PyPluginSdk>)>, String> {
                let lock = self
                    .python_runtime
                    .lock()
                    .map_err(|_| "python runtime lock poisoned".to_string())?;
                Ok(lock
                    .plugins
                    .iter()
                    .filter_map(|(plugin_id, plugin)| {
                        if disabled_declared_command_for_plugin(
                            &lock,
                            plugin_id.as_str(),
                            &event.payload,
                        )
                        .is_some()
                        {
                            return None;
                        }
                        plugin.event_handler.as_ref().map(|handler| {
                            (
                                plugin_id.clone(),
                                handler.clone_ref(py),
                                plugin.sdk.clone_ref(py),
                            )
                        })
                    })
                    .collect())
            },
        ) {
            Ok(handlers) => handlers,
            Err(_) => {
                logger.warn_in(
                    "plugin.python",
                    "python runtime lock poisoned, skip event dispatch",
                );
                return;
            }
        };
        if handlers.is_empty() {
            return;
        }

        let event_value = serde_json::json!({
            "id": event.id,
            "topic": event.topic,
            "payload": event.payload,
            "timestamp_ms": event.timestamp_ms,
        });
        for (plugin_id, handler, sdk) in handlers {
            let dispatch_result = Python::with_gil(|py| -> PyResult<()> {
                let event_obj = json_to_pyobject(py, &event_value)?;
                let callable = handler.bind(py);
                let sdk_obj: Py<PyAny> = sdk.clone_ref(py).into_any();
                let result = call_python_callable_with_fallback(
                    py,
                    &callable,
                    vec![
                        vec![event_obj.clone_ref(py), sdk_obj.clone_ref(py)],
                        vec![event_obj],
                        vec![sdk_obj],
                        Vec::new(),
                    ],
                )?;
                let _ = await_python_result(py, result)?;
                Ok(())
            });
            if let Err(err) = dispatch_result {
                logger.warn_in(
                    "plugin.python",
                    format!(
                        "python plugin '{}' event handler failed: {}",
                        plugin_id, err
                    ),
                );
            }
        }
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
        let Some(command) = normalize_tui_command_name(command) else {
            return Ok(None);
        };
        let Some((handler, sdk)) = Python::with_gil(
            |py| -> Result<Option<(Py<PyAny>, Py<PyPluginSdk>)>, PluginSdkError> {
                let lock = self.python_runtime.lock().map_err(|_| {
                    PluginSdkError::Runtime("python runtime lock poisoned".to_string())
                })?;
                let Some(command_entry) = lock.commands.get(command.as_str()) else {
                    return Ok(None);
                };
                if !command_entry.enabled
                    || is_scope_command_disabled(&lock, "tui", command_entry.command.as_str())
                {
                    return Err(PluginSdkError::Runtime(format!(
                        "plugin command '{}' is disabled",
                        command
                    )));
                }
                let sdk = lock
                    .plugins
                    .get(command_entry.plugin_id.as_str())
                    .map(|plugin| plugin.sdk.clone_ref(py))
                    .ok_or_else(|| {
                        PluginSdkError::Runtime(format!(
                            "plugin '{}' runtime not found for command '{}'",
                            command_entry.plugin_id, command
                        ))
                    })?;
                Ok(Some((command_entry.handler.clone_ref(py), sdk)))
            },
        )?
        else {
            return Ok(None);
        };

        let output = Python::with_gil(|py| -> PyResult<String> {
            let callable = handler.bind(py);
            let args_list = PyList::new(py, args)?.into_any().unbind();
            let sdk_obj: Py<PyAny> = sdk.clone_ref(py).into_any();
            let result = call_python_callable_with_fallback(
                py,
                &callable,
                vec![
                    vec![args_list.clone_ref(py), sdk_obj.clone_ref(py)],
                    vec![args_list],
                    vec![sdk_obj],
                    Vec::new(),
                ],
            )?;
            let awaited = await_python_result(py, result)?;
            render_python_command_result(py, awaited, command.as_str())
        })
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "plugin command '{}' execution failed: {}",
                command, err
            ))
        })?;

        Ok(Some(output))
    }

    fn load_python_manifest_plugin(
        &self,
        descriptor: &PluginDescriptor,
        host: &PluginHostBridge,
    ) -> Result<bool, PluginSdkError> {
        let probe = match probe_python_plugin_compatibility(descriptor) {
            Ok(probe) => probe,
            Err(_) => return Ok(false),
        };
        let plugin_id = descriptor.metadata.id.clone();
        let runtime_state = self.python_runtime.clone();
        let host = host.clone();
        let runtime_options = descriptor.runtime.options.clone();
        let permissions = PluginPermissionSet::from_declared(descriptor.permissions.as_slice())
            .map_err(PluginSdkError::Runtime)?;
        let event_handler_override = runtime_options
            .get("event_handler")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .map(ToString::to_string);
        let start_handler_override = runtime_options
            .get("start_handler")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .map(ToString::to_string);
        let health_handler_override = runtime_options
            .get("health_handler")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .map(ToString::to_string);
        let shutdown_handler_override = runtime_options
            .get("shutdown_handler")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .map(ToString::to_string);
        let unload_handler_override = runtime_options
            .get("unload_handler")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .map(ToString::to_string);
        let config_path = runtime_options
            .get("config_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .map(PathBuf::from);

        Python::with_gil(|py| -> PyResult<()> {
            ensure_python_search_paths(py, probe.search_paths.as_slice())?;

            let sdk = Py::new(
                py,
                PyPluginSdk::new(
                    plugin_id.clone(),
                    host.clone(),
                    runtime_state.clone(),
                    config_path.clone(),
                    permissions.clone(),
                ),
            )?;
            install_python_sdk_bridge(py, Some(&sdk))?;
            let module = PyModule::import(py, probe.entrypoint.module.as_str())?;
            inspect_python_legacy_metadata(&module);

            {
                let mut lock = runtime_state
                    .lock()
                    .map_err(|_| PyRuntimeError::new_err("python runtime lock poisoned"))?;
                remove_plugin_runtime_state(&mut lock, plugin_id.as_str());
            }

            invoke_python_bootstrap(py, &module, &probe.entrypoint, &sdk)?;
            let event_handler = resolve_python_event_handler(&module, event_handler_override)?;
            let start_handler = resolve_python_lifecycle_handler(
                &module,
                start_handler_override,
                &PYTHON_START_HANDLER_ATTRS,
            )?;
            let health_handler = resolve_python_lifecycle_handler(
                &module,
                health_handler_override,
                &PYTHON_HEALTH_HANDLER_ATTRS,
            )?;
            let shutdown_handler = resolve_python_lifecycle_handler(
                &module,
                shutdown_handler_override,
                &PYTHON_SHUTDOWN_HANDLER_ATTRS,
            )?;
            let unload_handler = resolve_python_lifecycle_handler(
                &module,
                unload_handler_override,
                &PYTHON_UNLOAD_HANDLER_ATTRS,
            )?;

            let mut lock = runtime_state
                .lock()
                .map_err(|_| PyRuntimeError::new_err("python runtime lock poisoned"))?;
            register_declared_commands(
                &mut lock.declared_commands,
                plugin_id.as_str(),
                descriptor.commands.as_slice(),
            );
            lock.plugins.insert(
                plugin_id.clone(),
                PythonLoadedPlugin {
                    event_handler,
                    start_handler,
                    health_handler,
                    shutdown_handler,
                    unload_handler,
                    sdk,
                },
            );
            Ok(())
        })
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' load failed: {}",
                plugin_id, err
            ))
        })?;

        Ok(true)
    }
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

#[derive(Debug, Clone)]
struct PythonEntrypoint {
    module: String,
    callable: Option<String>,
}

#[derive(Debug, Clone)]
struct PythonCompatibilityProbe {
    entrypoint: PythonEntrypoint,
    search_paths: Vec<PathBuf>,
}

fn probe_python_plugin_compatibility(
    descriptor: &PluginDescriptor,
) -> Result<PythonCompatibilityProbe, String> {
    let entrypoint = parse_python_entrypoint(descriptor)?;
    let search_paths = collect_python_search_paths(descriptor);
    Python::with_gil(|py| -> PyResult<()> {
        ensure_python_search_paths(py, search_paths.as_slice())?;
        install_python_sdk_bridge(py, None)?;
        let module = PyModule::import(py, entrypoint.module.as_str())?;
        inspect_python_legacy_metadata(&module);
        if let Some(callable_name) = entrypoint.callable.as_deref() {
            let target = module.getattr(callable_name)?;
            if !target.is_callable() {
                return Err(PyTypeError::new_err(format!(
                    "python entrypoint '{}' in module '{}' is not callable",
                    callable_name, entrypoint.module
                )));
            }
        }
        Ok(())
    })
    .map_err(|err| {
        format!(
            "pyo3 compatibility probe failed for '{}': {}",
            entrypoint.module, err
        )
    })?;
    Ok(PythonCompatibilityProbe {
        entrypoint,
        search_paths,
    })
}

fn parse_python_entrypoint(descriptor: &PluginDescriptor) -> Result<PythonEntrypoint, String> {
    let entrypoint = descriptor.runtime.entrypoint.trim();
    let module_hint = descriptor.runtime.module.trim();
    let raw = if !entrypoint.is_empty() {
        entrypoint
    } else if !module_hint.is_empty() {
        module_hint
    } else {
        return Err(
            "python plugin entrypoint/module is empty; expected `module[:callable]`".to_string(),
        );
    };

    let (module, callable) = match raw.split_once(':') {
        Some((module, callable)) => (
            module.trim().to_string(),
            Some(callable.trim().to_string()).filter(|name| !name.is_empty()),
        ),
        None => (raw.to_string(), None),
    };

    if module.trim().is_empty() {
        return Err("python plugin module name is empty".to_string());
    }
    if raw.contains(':') && callable.is_none() {
        return Err("python plugin callable name is empty".to_string());
    }

    Ok(PythonEntrypoint { module, callable })
}

fn collect_python_search_paths(descriptor: &PluginDescriptor) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let manifest_dir = descriptor
        .manifest_path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    if let Some(dir) = &manifest_dir {
        push_unique_path(&mut paths, dir.clone());
        if let Some(parent) = dir.parent() {
            push_unique_path(&mut paths, parent.to_path_buf());
        }
    }

    for key in ["python_path", "python_paths", "sys_path"] {
        if let Some(value) = descriptor.runtime.options.get(key) {
            extend_python_paths_from_value(value, manifest_dir.as_deref(), &mut paths);
        }
    }

    paths
}

fn extend_python_paths_from_value(value: &Value, base: Option<&Path>, output: &mut Vec<PathBuf>) {
    match value {
        Value::String(raw) => {
            for item in split_python_path_list(raw) {
                push_unique_path(output, resolve_python_path(item, base));
            }
        }
        Value::Array(list) => {
            for item in list {
                if let Some(raw) = item.as_str() {
                    for path in split_python_path_list(raw) {
                        push_unique_path(output, resolve_python_path(path, base));
                    }
                }
            }
        }
        _ => {}
    }
}

fn split_python_path_list(raw: &str) -> Vec<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let split_paths: Vec<PathBuf> = env::split_paths(trimmed).collect();
    if split_paths.len() > 1 {
        return split_paths;
    }

    if trimmed.contains(',') {
        let parts: Vec<PathBuf> = trimmed
            .split(',')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(PathBuf::from)
            .collect();
        if !parts.is_empty() {
            return parts;
        }
    }

    vec![PathBuf::from(trimmed)]
}

fn resolve_python_path(path: PathBuf, base: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path
    } else if let Some(base) = base {
        base.join(path)
    } else {
        path
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if candidate.as_os_str().is_empty() {
        return;
    }
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
    }
}

fn ensure_python_search_paths(py: Python<'_>, paths: &[PathBuf]) -> PyResult<()> {
    let sys = py.import("sys")?;
    let py_path = sys.getattr("path")?;
    for path in paths.iter().rev() {
        let path_text = path.to_string_lossy().into_owned();
        if path_text.trim().is_empty() {
            continue;
        }
        let exists = py_path
            .call_method1("__contains__", (path_text.as_str(),))?
            .is_truthy()?;
        if !exists {
            py_path.call_method1("insert", (0, path_text.as_str()))?;
        }
    }
    Ok(())
}

fn inspect_python_legacy_metadata(module: &pyo3::Bound<'_, PyModule>) {
    for attr in PYTHON_META_ATTRS {
        if let Ok(meta) = module.getattr(attr) {
            let _ = meta
                .getattr("name")
                .and_then(|name| name.extract::<String>());
            let _ = meta.getattr("type").and_then(|kind| {
                if let Ok(value) = kind.getattr("value") {
                    value.extract::<String>()
                } else {
                    kind.extract::<String>()
                }
            });
            break;
        }
    }
}

fn invoke_python_bootstrap(
    py: Python<'_>,
    module: &pyo3::Bound<'_, PyModule>,
    entrypoint: &PythonEntrypoint,
    sdk: &Py<PyPluginSdk>,
) -> PyResult<()> {
    let callable = if let Some(callable_name) = entrypoint.callable.as_deref() {
        Some(module.getattr(callable_name)?)
    } else if let Ok(attr) = module.getattr("on_load") {
        if attr.is_callable() { Some(attr) } else { None }
    } else {
        None
    };
    let Some(callable) = callable else {
        return Ok(());
    };
    let sdk_obj: Py<PyAny> = sdk.clone_ref(py).into_any();
    let result =
        call_python_callable_with_fallback(py, &callable, vec![vec![sdk_obj], Vec::new()])?;
    let _ = await_python_result(py, result)?;
    Ok(())
}

fn resolve_python_event_handler(
    module: &pyo3::Bound<'_, PyModule>,
    override_name: Option<String>,
) -> PyResult<Option<Py<PyAny>>> {
    if let Some(name) = override_name {
        let handler = module.getattr(name.as_str())?;
        if !handler.is_callable() {
            return Err(PyTypeError::new_err(format!(
                "python event handler '{}' is not callable",
                name
            )));
        }
        return Ok(Some(handler.unbind().into()));
    }
    for candidate in PYTHON_EVENT_HANDLER_ATTRS {
        if let Ok(handler) = module.getattr(candidate) {
            if handler.is_callable() {
                return Ok(Some(handler.unbind().into()));
            }
        }
    }
    Ok(None)
}

fn resolve_python_lifecycle_handler(
    module: &pyo3::Bound<'_, PyModule>,
    override_name: Option<String>,
    defaults: &[&str],
) -> PyResult<Option<Py<PyAny>>> {
    if let Some(name) = override_name {
        let handler = module.getattr(name.as_str())?;
        if !handler.is_callable() {
            return Err(PyTypeError::new_err(format!(
                "python lifecycle handler '{}' is not callable",
                name
            )));
        }
        return Ok(Some(handler.unbind().into()));
    }
    for candidate in defaults {
        if let Ok(handler) = module.getattr(candidate) {
            if handler.is_callable() {
                return Ok(Some(handler.unbind().into()));
            }
        }
    }
    Ok(None)
}

fn install_python_sdk_bridge(py: Python<'_>, sdk: Option<&Py<PyPluginSdk>>) -> PyResult<()> {
    let sdk_module = PyModule::new(py, "liteyuki_sdk")?;
    if let Some(sdk) = sdk {
        sdk_module.add("sdk", sdk.clone_ref(py))?;
    } else {
        sdk_module.add("sdk", py.None())?;
    }

    let compat_code = r#"
from dataclasses import dataclass, field

class PluginType:
    APPLICATION = "application"
    SERVICE = "service"
    MODULE = "module"
    UNCLASSIFIED = "unclassified"
    TEST = "test"

@dataclass
class PluginMetadata:
    name: str
    description: str = ""
    usage: str = ""
    type: str = PluginType.UNCLASSIFIED
    author: str = ""
    homepage: str = ""
    extra: dict = field(default_factory=dict)

def _is_awaitable(value):
    return hasattr(value, "__await__")

def _normalize_prefixes(prefixes):
    if isinstance(prefixes, str):
        return [prefixes]
    if isinstance(prefixes, (list, tuple, set)):
        out = []
        for item in prefixes:
            text = str(item).strip()
            if text:
                out.append(text)
        return out
    return []

def _extract_message_text(payload):
    if not isinstance(payload, dict):
        return ""
    for key in ("raw_message", "text"):
        value = payload.get(key)
        if isinstance(value, str):
            return value
    message = payload.get("message")
    if isinstance(message, str):
        return message
    if isinstance(message, list):
        parts = []
        for segment in message:
            if isinstance(segment, str):
                parts.append(segment)
                continue
            if not isinstance(segment, dict):
                continue
            data = segment.get("data")
            if isinstance(data, dict):
                text = data.get("text")
                if isinstance(text, str):
                    parts.append(text)
                    continue
            text = segment.get("text")
            if isinstance(text, str):
                parts.append(text)
        return "".join(parts)
    return ""

class MessageEvent:
    def __init__(self, event, sdk=None):
        self._event = event if isinstance(event, dict) else {}
        self._payload = self._event.get("payload", {})
        if not isinstance(self._payload, dict):
            self._payload = {}
        self._sdk = sdk
        self.raw_message = _extract_message_text(self._payload)

    @property
    def payload(self):
        return self._payload

    def reply(self, message):
        if self._sdk is None:
            return False
        text = str(message).strip()
        if not text:
            return False
        if hasattr(self._sdk, "reply_text"):
            try:
                return bool(self._sdk.reply_text(self._event, text))
            except Exception:
                return False
        if hasattr(self._sdk, "log"):
            self._sdk.log(text)
        return False

async def _dispatch_legacy_handlers(event, sdk, module_globals):
    handlers = module_globals.get("__liteyuki_legacy_handlers__", [])
    message_event = MessageEvent(event, sdk)
    for item in list(handlers):
        handler = item.get("handler")
        prefixes = item.get("prefixes", [])
        rule = item.get("rule")
        if not callable(handler):
            continue
        if prefixes and not any(_legacy_startswith(message_event.raw_message, prefix) for prefix in prefixes):
            continue
        if callable(rule):
            try:
                allowed = rule(message_event)
                if _is_awaitable(allowed):
                    allowed = await allowed
                if not bool(allowed):
                    continue
            except Exception:
                continue
        result = handler(message_event)
        if _is_awaitable(result):
            await result

def _ensure_legacy_dispatcher(module_globals):
    if "liteyuki_handle_event" in module_globals:
        return
    async def _compat_dispatch(event, sdk=None):
        await _dispatch_legacy_handlers(event, sdk, module_globals)
    module_globals["liteyuki_handle_event"] = _compat_dispatch

class _OnStartswith:
    def __init__(self, prefixes, rule=None):
        self._prefixes = _normalize_prefixes(prefixes)
        self._rule = rule

    def handle(self):
        def decorator(func):
            module_globals = getattr(func, "__globals__", {})
            handlers = module_globals.setdefault("__liteyuki_legacy_handlers__", [])
            handlers.append({
                "kind": "startswith",
                "prefixes": self._prefixes,
                "rule": self._rule,
                "handler": func
            })
            _ensure_legacy_dispatcher(module_globals)
            return func
        return decorator

def on_startswith(prefixes, rule=None):
    return _OnStartswith(prefixes, rule=rule)

def _legacy_startswith(raw_message, prefix):
    raw_message = str(raw_message or "")
    prefix = str(prefix or "").strip()
    if not prefix:
        return False
    if raw_message.startswith(prefix):
        return True
    if prefix.startswith("/"):
        return False
    return raw_message.startswith("/" + prefix)

def is_su_rule(event):
    return True
"#;
    let root_module = PyModule::new(py, "liteyuki")?;
    let root_dict = root_module.dict();
    let builtins = py.import("builtins")?;
    builtins
        .getattr("exec")?
        .call1((compat_code, &root_dict, &root_dict))?;

    let plugin_module = PyModule::new(py, "liteyuki.plugin")?;
    plugin_module.add("PluginType", root_module.getattr("PluginType")?)?;
    plugin_module.add("PluginMetadata", root_module.getattr("PluginMetadata")?)?;

    let session_module = PyModule::new(py, "liteyuki.session")?;
    let session_on_module = PyModule::new(py, "liteyuki.session.on")?;
    session_on_module.add("on_startswith", root_module.getattr("on_startswith")?)?;
    let session_event_module = PyModule::new(py, "liteyuki.session.event")?;
    session_event_module.add("MessageEvent", root_module.getattr("MessageEvent")?)?;
    let session_rule_module = PyModule::new(py, "liteyuki.session.rule")?;
    session_rule_module.add("is_su_rule", root_module.getattr("is_su_rule")?)?;

    session_module.add("on", &session_on_module)?;
    session_module.add("event", &session_event_module)?;
    session_module.add("rule", &session_rule_module)?;

    root_module.add("plugin", &plugin_module)?;
    root_module.add("session", &session_module)?;
    if let Some(sdk) = sdk {
        root_module.add("sdk", sdk.clone_ref(py))?;
    } else {
        root_module.add("sdk", py.None())?;
    }

    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    modules.set_item("liteyuki_sdk", &sdk_module)?;
    modules.set_item("liteyuki", &root_module)?;
    modules.set_item("liteyuki.plugin", &plugin_module)?;
    modules.set_item("liteyuki.session", &session_module)?;
    modules.set_item("liteyuki.session.on", &session_on_module)?;
    modules.set_item("liteyuki.session.event", &session_event_module)?;
    modules.set_item("liteyuki.session.rule", &session_rule_module)?;
    Ok(())
}

fn call_python_callable_with_fallback(
    py: Python<'_>,
    callable: &pyo3::Bound<'_, PyAny>,
    variants: Vec<Vec<Py<PyAny>>>,
) -> PyResult<Py<PyAny>> {
    let mut last_type_error: Option<PyErr> = None;
    for args in variants {
        let result = callable.call1(PyTuple::new(py, args)?);
        match result {
            Ok(value) => return Ok(value.unbind()),
            Err(err) => {
                if err.is_instance_of::<PyTypeError>(py) {
                    last_type_error = Some(err);
                    continue;
                }
                return Err(err);
            }
        }
    }
    Err(last_type_error.unwrap_or_else(|| {
        PyTypeError::new_err("python callable invocation failed: no compatible signature")
    }))
}

fn await_python_result(py: Python<'_>, result: Py<PyAny>) -> PyResult<Py<PyAny>> {
    let result_ref = result.bind(py);
    let is_awaitable = result_ref.hasattr("__await__").unwrap_or(false);
    if !is_awaitable {
        return Ok(result);
    }
    let asyncio = py.import("asyncio")?;
    let awaited = asyncio.call_method1("run", (result_ref,))?;
    Ok(awaited.unbind())
}

fn render_python_command_result(
    py: Python<'_>,
    result: Py<PyAny>,
    command: &str,
) -> PyResult<String> {
    let value = result.bind(py);
    if value.is_none() {
        return Ok(format!("plugin command '{}' executed", command));
    }
    if let Ok(text) = value.extract::<String>() {
        if text.trim().is_empty() {
            return Ok(format!("plugin command '{}' executed", command));
        }
        return Ok(text);
    }
    if let Ok(json) = py_any_to_json(value) {
        return Ok(json.to_string());
    }
    let repr = value.repr()?.to_string();
    Ok(repr)
}

fn start_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<(), PluginSdkError> {
    let Some((handler, sdk)) = Python::with_gil(|py| -> Result<_, PluginSdkError> {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(None);
        };
        Ok(plugin
            .start_handler
            .as_ref()
            .map(|handler| (handler.clone_ref(py), plugin.sdk.clone_ref(py))))
    })?
    else {
        return Ok(());
    };

    Python::with_gil(|py| -> Result<(), PluginSdkError> {
        invoke_python_lifecycle_handler(py, &handler, &sdk).map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' start hook failed: {}",
                plugin_id, err
            ))
        })
    })
}

fn health_check_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<(), PluginSdkError> {
    let Some((handler, sdk)) = Python::with_gil(|py| -> Result<_, PluginSdkError> {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(None);
        };
        Ok(plugin
            .health_handler
            .as_ref()
            .map(|handler| (handler.clone_ref(py), plugin.sdk.clone_ref(py))))
    })?
    else {
        return Ok(());
    };

    Python::with_gil(|py| -> Result<(), PluginSdkError> {
        invoke_python_lifecycle_handler(py, &handler, &sdk).map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' health check failed: {}",
                plugin_id, err
            ))
        })
    })
}

fn shutdown_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<(), PluginSdkError> {
    Python::with_gil(|py| -> Result<(), PluginSdkError> {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(());
        };
        let Some(handler) = plugin.shutdown_handler.as_ref() else {
            return Ok(());
        };
        invoke_python_lifecycle_handler(py, &handler.clone_ref(py), &plugin.sdk.clone_ref(py))
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' shutdown hook failed: {}",
                    plugin_id, err
                ))
            })
    })
}

fn unload_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<(), PluginSdkError> {
    let hook_result = Python::with_gil(|py| -> Result<(), PluginSdkError> {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(());
        };
        let Some(handler) = plugin.unload_handler.as_ref() else {
            return Ok(());
        };
        invoke_python_lifecycle_handler(py, &handler.clone_ref(py), &plugin.sdk.clone_ref(py))
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' unload hook failed: {}",
                    plugin_id, err
                ))
            })
    });

    let cleanup_result = {
        let mut lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        remove_plugin_runtime_state(&mut lock, plugin_id);
        Ok(())
    };

    match (hook_result, cleanup_result) {
        (_, Err(err)) => Err(err),
        (Err(err), Ok(())) => Err(err),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn invoke_python_lifecycle_handler(
    py: Python<'_>,
    handler: &Py<PyAny>,
    sdk: &Py<PyPluginSdk>,
) -> PyResult<()> {
    let sdk_obj: Py<PyAny> = sdk.clone_ref(py).into_any();
    let result =
        call_python_callable_with_fallback(py, handler.bind(py), vec![vec![sdk_obj], Vec::new()])?;
    let _ = await_python_result(py, result)?;
    Ok(())
}

fn remove_plugin_runtime_state(state: &mut PythonRuntimeState, plugin_id: &str) {
    state.plugins.remove(plugin_id);
    state
        .commands
        .retain(|_, command| command.plugin_id.as_str() != plugin_id);
    state
        .declared_commands
        .retain(|command| command.plugin_id.as_str() != plugin_id);
}

fn disabled_declared_command_for_plugin(
    state: &PythonRuntimeState,
    plugin_id: &str,
    payload: &Value,
) -> Option<String> {
    let scope = adapter_scope_for_payload(payload)?;
    let message = extract_payload_message_text(payload)?;
    state
        .declared_commands
        .iter()
        .find(|entry| {
            entry.plugin_id == plugin_id
                && is_scope_command_disabled(state, scope, entry.command.as_str())
                && plugin_scope_matches(&entry.scopes, scope)
                && declared_command_matches_message(entry.command.as_str(), message.as_str())
        })
        .map(|entry| entry.command.clone())
}

fn adapter_scope_for_payload(payload: &Value) -> Option<&'static str> {
    let object = payload.as_object()?;
    let protocol = object
        .get("_adapter_protocol")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let is_onebot_v11 = protocol.eq_ignore_ascii_case("onebot.v11")
        || object.contains_key("post_type")
        || object.contains_key("meta_event_type");
    if !is_onebot_v11 || object.get("post_type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    Some("adapter:onebot11")
}

fn extract_payload_message_text(payload: &Value) -> Option<String> {
    let object = payload.as_object()?;
    for key in ["raw_message", "text"] {
        if let Some(text) = object.get(key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    if let Some(text) = object.get("message").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let segments = object.get("message")?.as_array()?;
    let mut text = String::new();
    for segment in segments {
        if let Some(raw) = segment.as_str() {
            text.push_str(raw);
            continue;
        }
        let Some(segment_object) = segment.as_object() else {
            continue;
        };
        if let Some(data_text) = segment_object
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("text"))
            .and_then(Value::as_str)
        {
            text.push_str(data_text);
            continue;
        }
        if let Some(segment_text) = segment_object.get("text").and_then(Value::as_str) {
            text.push_str(segment_text);
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn declared_command_matches_message(command: &str, message: &str) -> bool {
    let Some(command) = normalize_tui_command_name(command) else {
        return false;
    };
    let message = message.trim();
    if matches_command_with_prefix(message, command.as_str()) {
        return true;
    }
    command
        .strip_prefix('/')
        .is_some_and(|bare| matches_command_with_prefix(message, bare))
}

fn matches_command_with_prefix(message: &str, command_prefix: &str) -> bool {
    let message = message.trim();
    if message.eq_ignore_ascii_case(command_prefix) {
        return true;
    }
    parse_command_argument(message, command_prefix).is_some()
}

fn parse_command_argument(message: &str, command_prefix: &str) -> Option<String> {
    let command_prefix = command_prefix.trim();
    if command_prefix.is_empty() {
        return None;
    }

    let message = message.trim();
    if message.eq_ignore_ascii_case(command_prefix) {
        return Some(String::new());
    }

    let remainder = message.strip_prefix(command_prefix)?;
    let mut chars = remainder.chars();
    if !chars.next().is_some_and(char::is_whitespace) {
        return None;
    }

    Some(remainder.trim().to_string())
}

fn normalize_tui_command_name(raw: &str) -> Option<String> {
    let first = raw.split_whitespace().next()?.trim();
    if first.is_empty() {
        return None;
    }
    let normalized = if first.starts_with('/') {
        first.to_ascii_lowercase()
    } else {
        format!("/{}", first.to_ascii_lowercase())
    };
    if normalized == "/" {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_plugin_scope(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let compact = trimmed.to_ascii_lowercase().replace([' ', '_', '-'], "");
    match compact.as_str() {
        "all" => Some("all".to_string()),
        "tui" => Some("tui".to_string()),
        "adapter:onebot11" | "adapter:onebotv11" | "adapteronebot11" | "onebot11" | "onebotv11" => {
            Some("adapter:onebot11".to_string())
        }
        _ => Some(trimmed.to_ascii_lowercase()),
    }
}

fn normalize_plugin_scopes(raw_scopes: &[String]) -> Vec<String> {
    let mut scopes = Vec::new();
    for scope in raw_scopes {
        if let Some(normalized) = normalize_plugin_scope(scope)
            && !scopes.iter().any(|existing| existing == &normalized)
        {
            scopes.push(normalized);
        }
    }
    if scopes.is_empty() {
        scopes.push("all".to_string());
    }
    scopes
}

fn plugin_scope_matches(scopes: &[String], scope: &str) -> bool {
    let Some(scope) = normalize_plugin_scope(scope) else {
        return false;
    };
    scopes
        .iter()
        .filter_map(|entry| normalize_plugin_scope(entry))
        .any(|entry| entry == "all" || entry == scope)
}

fn normalize_scoped_command_key(scope: &str, command: &str) -> Option<ScopedCommandKey> {
    Some(ScopedCommandKey {
        scope: normalize_plugin_scope(scope)?,
        command: normalize_tui_command_name(command)?,
    })
}

fn is_builtin_command_disabled_in_lock(
    lock: &PythonRuntimeState,
    scope: &str,
    command: &str,
) -> bool {
    normalize_scoped_command_key(scope, command)
        .is_some_and(|key| lock.disabled_scope_commands.contains(&key))
}

fn is_scope_command_disabled(state: &PythonRuntimeState, scope: &str, command: &str) -> bool {
    normalize_scoped_command_key(scope, command)
        .is_some_and(|key| state.disabled_scope_commands.contains(&key))
}

fn set_builtin_command_enabled_in_lock(
    lock: &mut PythonRuntimeState,
    scope: &str,
    command: &str,
    enabled: bool,
) -> Result<bool, String> {
    let Some(key) = normalize_scoped_command_key(scope, command) else {
        return Err("command scope or name is invalid".to_string());
    };
    if enabled {
        Ok(lock.disabled_scope_commands.remove(&key))
    } else {
        Ok(lock.disabled_scope_commands.insert(key))
    }
}

fn sync_disabled_scope_commands_in_lock(
    lock: &mut PythonRuntimeState,
    entries: &[String],
) -> Result<(), String> {
    let mut disabled = HashSet::new();
    for entry in entries {
        let Some((scope, command)) = parse_disabled_scope_command_entry(entry.as_str()) else {
            return Err(format!(
                "invalid disabled scope command entry '{}': expected '<scope> <name>'",
                entry.trim()
            ));
        };
        let Some(key) = normalize_scoped_command_key(scope.as_str(), command.as_str()) else {
            return Err(format!(
                "invalid disabled scope command entry '{}': expected '<scope> <name>'",
                entry.trim()
            ));
        };
        disabled.insert(key);
    }
    lock.disabled_scope_commands = disabled;
    Ok(())
}

fn list_disabled_scope_commands(lock: &PythonRuntimeState) -> Vec<String> {
    let mut entries = lock
        .disabled_scope_commands
        .iter()
        .map(|entry| format!("{} {}", entry.scope, entry.command))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn parse_disabled_scope_command_entry(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    let (scope, command) = raw.split_once(char::is_whitespace)?;
    Some((scope.trim().to_string(), command.trim().to_string()))
}

fn register_declared_commands(
    commands: &mut Vec<PythonDeclaredCommandEntry>,
    plugin_id: &str,
    descriptors: &[PluginCommandDescriptor],
) {
    commands.retain(|command| command.plugin_id.as_str() != plugin_id);
    commands.extend(descriptors.iter().filter_map(|descriptor| {
        let command = normalize_tui_command_name(descriptor.name.as_str())?;
        let description = descriptor.description.trim();
        Some(PythonDeclaredCommandEntry {
            command,
            description: if description.is_empty() {
                "plugin declared command".to_string()
            } else {
                description.to_string()
            },
            plugin_id: plugin_id.to_string(),
            scopes: normalize_plugin_scopes(&descriptor.scopes),
        })
    }));
}

fn register_tui_command(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    command: &str,
    description: Option<String>,
    enabled: bool,
    handler: Py<PyAny>,
    py: Python<'_>,
) -> Result<(), String> {
    let Some(command) = normalize_tui_command_name(command) else {
        return Err("plugin command name should not be empty".to_string());
    };
    if !handler.bind(py).is_callable() {
        return Err(format!("handler for '{}' is not callable", command));
    }

    let description = description
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| "python plugin command".to_string());
    let mut lock = state
        .lock()
        .map_err(|_| "python runtime lock poisoned".to_string())?;
    if let Some(existing) = lock.commands.get(command.as_str()) {
        if existing.plugin_id != plugin_id {
            return Err(format!(
                "plugin command '{}' already registered by '{}'",
                command, existing.plugin_id
            ));
        }
    }
    lock.commands.insert(
        command.clone(),
        PythonTuiCommandEntry {
            command: command.clone(),
            description,
            enabled,
            plugin_id: plugin_id.to_string(),
            handler,
        },
    );
    Ok(())
}

fn set_tui_command_enabled(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    command: &str,
    enabled: bool,
) -> Result<bool, String> {
    let Some(command) = normalize_tui_command_name(command) else {
        return Err("plugin command name should not be empty".to_string());
    };
    let mut lock = state
        .lock()
        .map_err(|_| "python runtime lock poisoned".to_string())?;
    if let Some(entry) = lock.commands.get_mut(command.as_str()) {
        if entry.plugin_id != plugin_id {
            return Err(format!(
                "plugin command '{}' belongs to '{}' and cannot be changed by '{}'",
                command, entry.plugin_id, plugin_id
            ));
        }
        entry.enabled = enabled;
        return Ok(true);
    }

    set_builtin_command_enabled_in_lock(&mut lock, "tui", command.as_str(), enabled)
}

fn set_scope_command_enabled(
    state: &Arc<Mutex<PythonRuntimeState>>,
    scope: &str,
    command: &str,
    enabled: bool,
) -> Result<usize, String> {
    let Some(scope) = normalize_plugin_scope(scope) else {
        return Err("command scope is invalid".to_string());
    };
    let Some(command) = normalize_tui_command_name(command) else {
        return Err("plugin command name should not be empty".to_string());
    };

    let mut lock = state
        .lock()
        .map_err(|_| "python runtime lock poisoned".to_string())?;
    let mut matched = lock.declared_commands.iter().any(|entry| {
        entry.command == command && plugin_scope_matches(&entry.scopes, scope.as_str())
    });

    if scope == "tui" {
        matched |= lock.commands.values().any(|entry| entry.command == command);
    }

    let changed =
        set_builtin_command_enabled_in_lock(&mut lock, scope.as_str(), command.as_str(), enabled)?;
    if matched || changed {
        Ok(usize::from(changed || matched))
    } else {
        Ok(0)
    }
}

fn remove_tui_command(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    command: &str,
) -> Result<bool, String> {
    let Some(command) = normalize_tui_command_name(command) else {
        return Err("plugin command name should not be empty".to_string());
    };
    let mut lock = state
        .lock()
        .map_err(|_| "python runtime lock poisoned".to_string())?;
    if let Some(entry) = lock.commands.get(command.as_str()) {
        if entry.plugin_id != plugin_id {
            return Err(format!(
                "plugin command '{}' belongs to '{}' and cannot be removed by '{}'",
                command, entry.plugin_id, plugin_id
            ));
        }
    }
    Ok(lock.commands.remove(command.as_str()).is_some())
}

fn list_tui_commands(state: &Arc<Mutex<PythonRuntimeState>>) -> Vec<PluginTuiCommand> {
    let Ok(lock) = state.lock() else {
        return Vec::new();
    };
    let mut commands: Vec<PluginTuiCommand> = lock
        .commands
        .values()
        .map(|entry| PluginTuiCommand {
            name: entry.command.clone(),
            description: entry.description.clone(),
            enabled: entry.enabled
                && !is_scope_command_disabled(&lock, "tui", entry.command.as_str()),
            plugin_id: entry.plugin_id.clone(),
        })
        .collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    commands
}

fn list_scope_commands(
    state: &Arc<Mutex<PythonRuntimeState>>,
    scope: &str,
) -> Vec<PluginScopedCommand> {
    let Some(scope) = normalize_plugin_scope(scope) else {
        return Vec::new();
    };

    let Ok(lock) = state.lock() else {
        return Vec::new();
    };

    let mut merged: HashMap<String, PluginScopedCommand> = HashMap::new();

    for entry in &lock.declared_commands {
        if !plugin_scope_matches(&entry.scopes, scope.as_str()) {
            continue;
        }
        let key = format!("{}::{}", entry.plugin_id, entry.command);
        merged.insert(
            key,
            PluginScopedCommand {
                name: entry.command.clone(),
                description: entry.description.clone(),
                enabled: !is_scope_command_disabled(&lock, scope.as_str(), entry.command.as_str()),
                plugin_id: entry.plugin_id.clone(),
                scopes: entry.scopes.clone(),
                executable_in_tui: false,
            },
        );
    }

    for entry in lock.commands.values() {
        let scopes = vec!["tui".to_string()];
        if !plugin_scope_matches(&scopes, scope.as_str()) {
            continue;
        }
        let key = format!("{}::{}", entry.plugin_id, entry.command);
        merged
            .entry(key)
            .and_modify(|existing| {
                existing.description = entry.description.clone();
                existing.enabled = entry.enabled
                    && !is_scope_command_disabled(&lock, "tui", entry.command.as_str());
                existing.executable_in_tui = true;
                if !existing.scopes.iter().any(|scope| scope == "tui") {
                    existing.scopes.push("tui".to_string());
                }
            })
            .or_insert_with(|| PluginScopedCommand {
                name: entry.command.clone(),
                description: entry.description.clone(),
                enabled: entry.enabled
                    && !is_scope_command_disabled(&lock, "tui", entry.command.as_str()),
                plugin_id: entry.plugin_id.clone(),
                scopes,
                executable_in_tui: true,
            });
    }

    let mut commands: Vec<PluginScopedCommand> = merged.into_values().collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name).then(a.plugin_id.cmp(&b.plugin_id)));
    commands
}

#[derive(Debug, Clone, Copy)]
enum ConfigFormat {
    Yaml,
    Toml,
}

fn read_config_value(config_path: Option<&Path>, key: &str) -> Result<Option<Value>, String> {
    let _guard = PLUGIN_CONFIG_RW_LOCK
        .lock()
        .map_err(|_| "plugin config lock poisoned".to_string())?;
    let (_, _, document) = read_plugin_config_document(config_path)?;
    let segments = parse_config_path_segments(key)?;
    Ok(get_value_from_path(&document, &segments).cloned())
}

fn write_config_value(config_path: Option<&Path>, key: &str, value: Value) -> Result<(), String> {
    let _guard = PLUGIN_CONFIG_RW_LOCK
        .lock()
        .map_err(|_| "plugin config lock poisoned".to_string())?;
    let (path, format, mut document) = read_plugin_config_document(config_path)?;
    let segments = parse_config_path_segments(key)?;
    set_value_at_path(&mut document, &segments, value)?;
    write_plugin_config_document(path.as_path(), format, &document)
}

fn delete_config_value(config_path: Option<&Path>, key: &str) -> Result<bool, String> {
    let _guard = PLUGIN_CONFIG_RW_LOCK
        .lock()
        .map_err(|_| "plugin config lock poisoned".to_string())?;
    let (path, format, mut document) = read_plugin_config_document(config_path)?;
    let segments = parse_config_path_segments(key)?;
    let changed = delete_value_at_path(&mut document, &segments);
    if changed {
        write_plugin_config_document(path.as_path(), format, &document)?;
    }
    Ok(changed)
}

fn read_plugin_config_document(
    config_path: Option<&Path>,
) -> Result<(PathBuf, ConfigFormat, Value), String> {
    let path = resolve_plugin_config_path(config_path);
    let format = detect_config_format(path.as_path());
    if !path.exists() {
        return Ok((path, format, Value::Object(Map::new())));
    }
    let content = fs::read_to_string(path.as_path())
        .map_err(|err| format!("failed to read config {}: {}", path.display(), err))?;
    let value = match format {
        ConfigFormat::Yaml => {
            let yaml_value =
                serde_yaml::from_str::<serde_yaml::Value>(content.as_str()).map_err(|err| {
                    format!("failed to parse yaml config {}: {}", path.display(), err)
                })?;
            serde_json::to_value(yaml_value)
                .map_err(|err| format!("failed to convert yaml config to json: {}", err))?
        }
        ConfigFormat::Toml => {
            let toml_value = toml::from_str::<toml::Value>(content.as_str()).map_err(|err| {
                format!("failed to parse toml config {}: {}", path.display(), err)
            })?;
            serde_json::to_value(toml_value)
                .map_err(|err| format!("failed to convert toml config to json: {}", err))?
        }
    };
    Ok((path, format, value))
}

fn write_plugin_config_document(
    path: &Path,
    format: ConfigFormat,
    value: &Value,
) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create config parent directory {}: {}",
                parent.display(),
                err
            )
        })?;
    }
    let content = match format {
        ConfigFormat::Yaml => serde_yaml::to_string(value)
            .map_err(|err| format!("failed to serialize yaml config: {}", err))?,
        ConfigFormat::Toml => toml::to_string_pretty(value)
            .map_err(|err| format!("failed to serialize toml config: {}", err))?,
    };
    fs::write(path, content)
        .map_err(|err| format!("failed to write config {}: {}", path.display(), err))
}

fn resolve_plugin_config_path(config_path: Option<&Path>) -> PathBuf {
    if let Some(path) = config_path {
        return path.to_path_buf();
    }
    if let Ok(path) = std::env::var("LY_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    for candidate in DEFAULT_PLUGIN_CONFIG_PATHS {
        let candidate = PathBuf::from(candidate);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("config.yaml")
}

fn detect_config_format(path: &Path) -> ConfigFormat {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("toml") => ConfigFormat::Toml,
        _ => ConfigFormat::Yaml,
    }
}

fn parse_config_path_segments(key: &str) -> Result<Vec<&str>, String> {
    let key = key.trim();
    if key.is_empty() {
        return Ok(Vec::new());
    }
    let segments: Vec<&str> = key
        .split('.')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return Err("config path should not be empty".to_string());
    }
    Ok(segments)
}

fn get_value_from_path<'a>(value: &'a Value, segments: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in segments {
        cursor = cursor.as_object()?.get(*segment)?;
    }
    Some(cursor)
}

fn set_value_at_path(value: &mut Value, segments: &[&str], patch: Value) -> Result<(), String> {
    if segments.is_empty() {
        *value = patch;
        return Ok(());
    }
    let mut cursor = value;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        if !cursor.is_object() {
            *cursor = Value::Object(Map::new());
        }
        let map = cursor
            .as_object_mut()
            .ok_or_else(|| "failed to convert config node to object".to_string())?;
        cursor = map
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !cursor.is_object() {
        *cursor = Value::Object(Map::new());
    }
    let map = cursor
        .as_object_mut()
        .ok_or_else(|| "failed to convert config node to object".to_string())?;
    map.insert(
        segments
            .last()
            .ok_or_else(|| "config path should not be empty".to_string())?
            .to_string(),
        patch,
    );
    Ok(())
}

fn delete_value_at_path(value: &mut Value, segments: &[&str]) -> bool {
    if segments.is_empty() {
        return false;
    }
    let mut cursor = value;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        let Some(map) = cursor.as_object_mut() else {
            return false;
        };
        let Some(next) = map.get_mut(*segment) else {
            return false;
        };
        cursor = next;
    }
    let Some(map) = cursor.as_object_mut() else {
        return false;
    };
    map.remove(
        *segments
            .last()
            .expect("segments is not empty by construction"),
    )
    .is_some()
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

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn py_any_to_json(value: &pyo3::Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    let py = value.py();
    let json = py.import("json")?;
    let dumped: String = json.call_method1("dumps", (value,))?.extract()?;
    serde_json::from_str::<Value>(dumped.as_str()).map_err(|err| {
        PyValueError::new_err(format!("python value is not json-serializable: {}", err))
    })
}

fn json_to_pyobject(py: Python<'_>, value: &Value) -> PyResult<PyObject> {
    let json = py.import("json")?;
    let dumped = serde_json::to_string(value)
        .map_err(|err| PyValueError::new_err(format!("json serialization failed: {}", err)))?;
    let parsed = json.call_method1("loads", (dumped,))?;
    Ok(parsed.unbind())
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
