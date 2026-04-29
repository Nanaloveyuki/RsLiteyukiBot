use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyModule, PyTuple};
use serde_json::Value;

use crate::plugin::sdk::config::{delete_config_value, read_config_value, write_config_value};
use crate::plugin::sdk::python::async_runtime::await_python_awaitable;
use crate::plugin::sdk::python::bridge_contract::{
    ASTRBOT_BIND_RUNTIME_FN, ASTRBOT_CLEANUP_RUNTIME_FN, ASTRBOT_REQUIRED_RUNTIME_ATTRS,
    PYTHON_BRIDGE_SDK_GLOBAL, PYTHON_ROOT_MODULE, PYTHON_SDK_MODULE,
};
use crate::plugin::sdk::python::commands::{
    list_tui_commands, register_tui_command, remove_tui_command, set_tui_command_enabled,
};
use crate::plugin::sdk::python::json_codec::{json_to_pyobject, py_any_to_json};
use crate::plugin::sdk::python::state::PythonRuntimeState;
use crate::plugin::sdk::{PluginHostBridge, PluginPermissionSet, PluginSdkError};

const PERMISSION_KV_READ: &str = "kv.read";
const PERMISSION_KV_WRITE: &str = "kv.write";
const PERMISSION_CHANNEL_PUBLISH: &str = "channel.publish";
const PERMISSION_ADAPTER_REPLY: &str = "adapter.reply";
const PERMISSION_CONFIG_READ: &str = "config.read";
const PERMISSION_CONFIG_WRITE: &str = "config.write";
const PERMISSION_COMMAND_TUI_READ: &str = "command.tui.read";
const PERMISSION_COMMAND_TUI_MANAGE: &str = "command.tui.manage";
fn python_compat_runtime() -> String {
    format!(
        "{}\n{}\n{}\n{}\n_install_python_compat_modules(globals().get(\"{}\"))\n",
        include_str!("compat_runtime.py"),
        include_str!("../liteyukibot/compat_runtime.py"),
        include_str!("../astrbot/compat_runtime.py"),
        include_str!("../neomofox/compat_runtime.py"),
        PYTHON_BRIDGE_SDK_GLOBAL,
    )
}

#[pyclass]
#[derive(Clone)]
pub(crate) struct PyPluginSdk {
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
        let message = crate::comm::ChannelMessage::new(topic, payload, Some("python-plugin"));
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
    pub(super) fn new(
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

pub(super) fn install_python_sdk_bridge(
    py: Python<'_>,
    sdk: Option<&Py<PyPluginSdk>>,
) -> PyResult<()> {
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    let sdk_module = match modules.get_item(PYTHON_SDK_MODULE)? {
        Some(existing) => existing.downcast_into::<PyModule>()?,
        None => {
            let module = PyModule::new(py, PYTHON_SDK_MODULE)?;
            modules.set_item(PYTHON_SDK_MODULE, &module)?;
            module
        }
    };
    let root_module = match modules.get_item(PYTHON_ROOT_MODULE)? {
        Some(existing) => existing.downcast_into::<PyModule>()?,
        None => {
            let module = PyModule::new(py, PYTHON_ROOT_MODULE)?;
            modules.set_item(PYTHON_ROOT_MODULE, &module)?;
            module
        }
    };

    let root_dict = root_module.dict();
    let builtins = py.import("builtins")?;
    if !python_bridge_runtime_installed(&root_module) {
        let compat_runtime = python_compat_runtime();
        builtins
            .getattr("exec")?
            .call1((compat_runtime.as_str(), &root_dict, &root_dict))?;
    }

    let sdk_value: PyObject = match sdk {
        Some(sdk) => sdk.clone_ref(py).into_any(),
        None => py.None(),
    };
    sdk_module.setattr("sdk", sdk_value.clone_ref(py))?;
    root_module.setattr("sdk", sdk_value.clone_ref(py))?;
    root_dict.set_item(PYTHON_BRIDGE_SDK_GLOBAL, sdk_value)?;
    Ok(())
}

fn python_bridge_runtime_installed(root_module: &pyo3::Bound<'_, PyModule>) -> bool {
    ASTRBOT_REQUIRED_RUNTIME_ATTRS.iter().all(|attr| {
        root_module
            .getattr(attr)
            .ok()
            .is_some_and(|value| value.is_callable())
    })
}

pub(super) fn bind_astrbot_plugin_runtime(
    py: Python<'_>,
    module: &pyo3::Bound<'_, PyModule>,
    sdk: &Py<PyPluginSdk>,
) -> PyResult<bool> {
    let liteyuki = PyModule::import(py, PYTHON_ROOT_MODULE)?;
    let binder = liteyuki.getattr(ASTRBOT_BIND_RUNTIME_FN)?;
    let result = binder.call1((module, sdk.clone_ref(py)))?;
    let result = await_python_result(py, result.unbind())?;
    result.bind(py).extract::<bool>()
}

pub(super) fn cleanup_astrbot_plugin_runtime(
    py: Python<'_>,
    module_names: &[String],
) -> PyResult<()> {
    if module_names.is_empty() {
        return Ok(());
    }
    let liteyuki = PyModule::import(py, PYTHON_ROOT_MODULE)?;
    let cleaner = liteyuki.getattr(ASTRBOT_CLEANUP_RUNTIME_FN)?;
    for module_name in module_names {
        cleaner.call1((module_name.as_str(),))?;
    }
    Ok(())
}

pub(super) fn call_python_callable_with_fallback(
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

pub(super) fn await_python_result(py: Python<'_>, result: Py<PyAny>) -> PyResult<Py<PyAny>> {
    let result_ref = result.bind(py);
    let is_awaitable = result_ref.hasattr("__await__").unwrap_or(false);
    if !is_awaitable {
        return Ok(result);
    }
    await_python_awaitable(py, result)
}

pub(super) fn render_python_command_result(
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

pub(super) fn plugin_runtime_error(plugin_id: &str, err: PyErr) -> PluginSdkError {
    PluginSdkError::Runtime(format!(
        "python plugin '{}' load failed: {}",
        plugin_id, err
    ))
}
