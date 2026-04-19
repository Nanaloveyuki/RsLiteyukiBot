use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex};

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyModule, PyTuple};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::comm::{ChannelMessage, ChannelRegistry, SharedStore};
use crate::core::{BotEvent, LifecycleContext};
use crate::observability::Logger;
use crate::session::SessionRouter;

use super::abi::PluginAbiContract;
use super::{PluginDescriptor, PluginRuntimeKind};

const PYTHON_META_ATTRS: [&str; 3] = [
    "__plugin_meta__",
    "__plugin_metadata__",
    "__liteyuki_plugin_meta__",
];
const PYTHON_EVENT_HANDLER_ATTRS: [&str; 3] = ["on_event", "handle_event", "liteyuki_handle_event"];
const DEFAULT_PLUGIN_CONFIG_PATHS: [&str; 6] = [
    "config.yaml",
    "rust-config.yaml",
    "rust-config.yml",
    "rust-config.toml",
    "config/rust-core.yaml",
    "config/rust-core.toml",
];
static PLUGIN_CONFIG_RW_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

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
}

#[derive(Clone)]
pub struct PluginHostBridge {
    lifecycle: Arc<LifecycleContext>,
    channels: ChannelRegistry,
    shared_store: SharedStore,
    session_router: SessionRouter,
    logger: Logger,
}

impl PluginHostBridge {
    pub fn new(
        lifecycle: Arc<LifecycleContext>,
        channels: ChannelRegistry,
        shared_store: SharedStore,
        session_router: SessionRouter,
        logger: Logger,
    ) -> Self {
        Self {
            lifecycle,
            channels,
            shared_store,
            session_router,
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

    pub fn logger(&self) -> &Logger {
        &self.logger
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

#[derive(Default)]
struct PythonRuntimeState {
    plugins: HashMap<String, PythonLoadedPlugin>,
    commands: HashMap<String, PythonTuiCommandEntry>,
    disabled_builtin_commands: HashSet<String>,
}

struct PythonLoadedPlugin {
    event_handler: Option<Py<PyAny>>,
    sdk: Py<PyPluginSdk>,
}

struct PythonTuiCommandEntry {
    command: String,
    description: String,
    enabled: bool,
    plugin_id: String,
    handler: Py<PyAny>,
}

#[pyclass]
#[derive(Clone)]
struct PyPluginSdk {
    plugin_id: String,
    host: PluginHostBridge,
    runtime_state: Arc<Mutex<PythonRuntimeState>>,
    config_path: Option<PathBuf>,
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
        let value = self
            .host
            .shared_store()
            .get(key.trim())
            .unwrap_or(Value::Null);
        json_to_pyobject(py, &value)
    }

    fn kv_set(&self, py: Python<'_>, key: String, value: Py<PyAny>) -> PyResult<()> {
        let key = key.trim();
        if key.is_empty() {
            return Err(PyValueError::new_err("kv key should not be empty"));
        }
        let value = py_any_to_json(value.bind(py))?;
        self.host.shared_store().set(key.to_string(), value);
        Ok(())
    }

    fn kv_delete(&self, key: String) -> bool {
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

    fn config_get(&self, py: Python<'_>, key: String) -> PyResult<PyObject> {
        let value = read_config_value(self.config_path.as_deref(), key.as_str())
            .map_err(PyRuntimeError::new_err)?
            .unwrap_or(Value::Null);
        json_to_pyobject(py, &value)
    }

    fn config_set(&self, py: Python<'_>, key: String, value: Py<PyAny>) -> PyResult<()> {
        let value = py_any_to_json(value.bind(py))?;
        write_config_value(self.config_path.as_deref(), key.as_str(), value)
            .map_err(PyRuntimeError::new_err)
    }

    fn config_delete(&self, key: String) -> PyResult<bool> {
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
        set_tui_command_enabled(
            &self.runtime_state,
            self.plugin_id.as_str(),
            command.as_str(),
            false,
        )
        .map_err(PyRuntimeError::new_err)
    }

    fn enable_tui_command(&self, command: String) -> PyResult<bool> {
        set_tui_command_enabled(
            &self.runtime_state,
            self.plugin_id.as_str(),
            command.as_str(),
            true,
        )
        .map_err(PyRuntimeError::new_err)
    }

    fn remove_tui_command(&self, command: String) -> PyResult<bool> {
        remove_tui_command(
            &self.runtime_state,
            self.plugin_id.as_str(),
            command.as_str(),
        )
        .map_err(PyRuntimeError::new_err)
    }

    fn list_tui_commands(&self, py: Python<'_>) -> PyResult<PyObject> {
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
    ) -> Self {
        Self {
            plugin_id,
            host,
            runtime_state,
            config_path,
        }
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
        let Some(command) = normalize_tui_command_name(command) else {
            return false;
        };
        self.python_runtime
            .lock()
            .map(|lock| lock.disabled_builtin_commands.contains(command.as_str()))
            .unwrap_or(false)
    }

    pub fn list_tui_commands(&self) -> Vec<PluginTuiCommand> {
        list_tui_commands(&self.python_runtime)
    }

    pub fn get_tui_command(&self, command: &str) -> Option<PluginTuiCommand> {
        let command = normalize_tui_command_name(command)?;
        self.python_runtime.lock().ok().and_then(|lock| {
            lock.commands
                .get(command.as_str())
                .map(|entry| PluginTuiCommand {
                    name: entry.command.clone(),
                    description: entry.description.clone(),
                    enabled: entry.enabled,
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
                if !command_entry.enabled {
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
        let event_handler_override = runtime_options
            .get("event_handler")
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
            let module = PyModule::import(py, probe.entrypoint.module.as_str())?;
            inspect_python_legacy_metadata(&module);

            let sdk = Py::new(
                py,
                PyPluginSdk::new(
                    plugin_id.clone(),
                    host.clone(),
                    runtime_state.clone(),
                    config_path.clone(),
                ),
            )?;
            install_python_sdk_bridge(py, &sdk)?;

            {
                let mut lock = runtime_state
                    .lock()
                    .map_err(|_| PyRuntimeError::new_err("python runtime lock poisoned"))?;
                remove_plugin_runtime_state(&mut lock, plugin_id.as_str());
            }

            invoke_python_bootstrap(py, &module, &probe.entrypoint, &sdk)?;
            let event_handler = resolve_python_event_handler(&module, event_handler_override)?;

            let mut lock = runtime_state
                .lock()
                .map_err(|_| PyRuntimeError::new_err("python runtime lock poisoned"))?;
            lock.plugins
                .insert(plugin_id.clone(), PythonLoadedPlugin { event_handler, sdk });
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
        _host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let abi_version = normalize_abi_version(&descriptor.runtime.abi);
        let host_api_version = normalize_host_api_version(&descriptor.sdk.api_version);
        let mut contract = PluginAbiContract::new(
            PluginRuntimeKind::Native,
            "liteyuki-native",
            abi_version,
            host_api_version,
        );
        contract
            .required_methods
            .push(super::abi::PluginAbiMethod::HandleEvent);
        let has_entry = !descriptor.runtime.entrypoint.trim().is_empty()
            || !descriptor.runtime.module.trim().is_empty();
        Box::pin(async move {
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
        _host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let mut contract = PluginAbiContract::new(
            PluginRuntimeKind::Python,
            "liteyuki-python-bridge",
            normalize_abi_version(&descriptor.runtime.abi),
            normalize_host_api_version(&descriptor.sdk.api_version),
        );
        contract
            .required_methods
            .push(super::abi::PluginAbiMethod::HandleEvent);
        let probe = probe_python_plugin_compatibility(descriptor);
        Box::pin(async move {
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
        _host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = PluginAbiContract::new(
            PluginRuntimeKind::Lua,
            "liteyuki-lua-bridge",
            normalize_abi_version(&descriptor.runtime.abi),
            normalize_host_api_version(&descriptor.sdk.api_version),
        );
        Box::pin(async move {
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

fn install_python_sdk_bridge(py: Python<'_>, sdk: &Py<PyPluginSdk>) -> PyResult<()> {
    let sdk_module = PyModule::new(py, "liteyuki_sdk")?;
    sdk_module.add("sdk", sdk.clone_ref(py))?;

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
"#;
    let plugin_module = PyModule::new(py, "liteyuki.plugin")?;
    let plugin_dict = plugin_module.dict();
    let builtins = py.import("builtins")?;
    builtins
        .getattr("exec")?
        .call1((compat_code, &plugin_dict, &plugin_dict))?;
    let root_module = PyModule::new(py, "liteyuki")?;
    root_module.add("plugin", &plugin_module)?;
    root_module.add("sdk", sdk.clone_ref(py))?;

    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    modules.set_item("liteyuki_sdk", sdk_module)?;
    modules.set_item("liteyuki", root_module)?;
    modules.set_item("liteyuki.plugin", plugin_module)?;
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

fn remove_plugin_runtime_state(state: &mut PythonRuntimeState, plugin_id: &str) {
    state.plugins.remove(plugin_id);
    state
        .commands
        .retain(|_, command| command.plugin_id.as_str() != plugin_id);
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
    lock.disabled_builtin_commands.remove(command.as_str());
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

    if enabled {
        Ok(lock.disabled_builtin_commands.remove(command.as_str()))
    } else {
        lock.disabled_builtin_commands.insert(command);
        Ok(true)
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
            enabled: entry.enabled,
            plugin_id: entry.plugin_id.clone(),
        })
        .collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name));
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

fn normalize_host_api_version(raw: &str) -> String {
    if raw.trim().is_empty() {
        "0.1".to_string()
    } else {
        raw.trim().to_string()
    }
}
