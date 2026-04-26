use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyModule, PyTuple};
use serde_json::Value;

use crate::plugin::sdk::config::{delete_config_value, read_config_value, write_config_value};
use crate::plugin::sdk::python::async_runtime::await_python_awaitable;
use crate::plugin::sdk::python::commands::{
    list_tui_commands, register_tui_command, remove_tui_command, set_tui_command_enabled,
};
use crate::plugin::sdk::python::lifecycle::PythonRuntimeState;
use crate::plugin::sdk::{PluginHostBridge, PluginPermissionSet, PluginSdkError};

const PERMISSION_KV_READ: &str = "kv.read";
const PERMISSION_KV_WRITE: &str = "kv.write";
const PERMISSION_CHANNEL_PUBLISH: &str = "channel.publish";
const PERMISSION_ADAPTER_REPLY: &str = "adapter.reply";
const PERMISSION_CONFIG_READ: &str = "config.read";
const PERMISSION_CONFIG_WRITE: &str = "config.write";
const PERMISSION_COMMAND_TUI_READ: &str = "command.tui.read";
const PERMISSION_COMMAND_TUI_MANAGE: &str = "command.tui.manage";
const PYTHON_COMPAT_RUNTIME: &str = concat!(
    include_str!("compat_runtime.py"),
    "\n",
    include_str!("../liteyukibot/compat_runtime.py"),
    "\n",
    include_str!("../astrbot/compat_runtime.py"),
    "\n",
    include_str!("../neomofox/compat_runtime.py"),
    "\n_install_python_compat_modules(globals().get(\"__bridge_sdk__\"))\n",
);

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
    let sdk_module = match modules.get_item("liteyuki_sdk")? {
        Some(existing) => existing.downcast_into::<PyModule>()?,
        None => {
            let module = PyModule::new(py, "liteyuki_sdk")?;
            modules.set_item("liteyuki_sdk", &module)?;
            module
        }
    };
    let root_module = match modules.get_item("liteyuki")? {
        Some(existing) => existing.downcast_into::<PyModule>()?,
        None => {
            let module = PyModule::new(py, "liteyuki")?;
            modules.set_item("liteyuki", &module)?;
            module
        }
    };

    let root_dict = root_module.dict();
    let builtins = py.import("builtins")?;
    if root_module.getattr("_bind_astrbot_plugin_runtime").is_err() {
        builtins
            .getattr("exec")?
            .call1((PYTHON_COMPAT_RUNTIME, &root_dict, &root_dict))?;
    }

    let sdk_value: PyObject = match sdk {
        Some(sdk) => sdk.clone_ref(py).into_any(),
        None => py.None(),
    };
    sdk_module.setattr("sdk", sdk_value.clone_ref(py))?;
    root_module.setattr("sdk", sdk_value.clone_ref(py))?;
    root_dict.set_item("__bridge_sdk__", sdk_value)?;
    Ok(())
}

pub(super) fn bind_astrbot_plugin_runtime(
    py: Python<'_>,
    module: &pyo3::Bound<'_, PyModule>,
    sdk: &Py<PyPluginSdk>,
) -> PyResult<bool> {
    let liteyuki = PyModule::import(py, "liteyuki")?;
    let binder = liteyuki.getattr("_bind_astrbot_plugin_runtime")?;
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
    let liteyuki = PyModule::import(py, "liteyuki")?;
    let cleaner = liteyuki.getattr("_cleanup_astrbot_plugin_runtime")?;
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

pub(super) fn capture_plugin_module_names(
    py: Python<'_>,
    entry_module: &str,
    search_paths: &[PathBuf],
) -> PyResult<Vec<String>> {
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    let builtins = py.import("builtins")?;
    let items = builtins
        .getattr("list")?
        .call1((modules.items(),))?
        .downcast_into::<PyList>()?;
    let mut names = HashSet::new();
    let entry_prefix = format!("{entry_module}.");

    for entry in items.iter() {
        let tuple = entry.downcast_into::<PyTuple>()?;
        let Some(key) = tuple.get_item(0).ok() else {
            continue;
        };
        let Some(value) = tuple.get_item(1).ok() else {
            continue;
        };
        let Ok(name) = key.extract::<String>() else {
            continue;
        };
        if name == entry_module || name.starts_with(entry_prefix.as_str()) {
            names.insert(name);
            continue;
        }
        if module_matches_search_paths(&value, search_paths)? {
            names.insert(name);
        }
    }

    let mut modules = names.into_iter().collect::<Vec<_>>();
    modules.sort();
    Ok(modules)
}

pub(super) fn remove_stale_entrypoint_modules(
    py: Python<'_>,
    entry_module: &str,
    search_paths: &[PathBuf],
) -> PyResult<()> {
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    let builtins = py.import("builtins")?;
    let items = builtins
        .getattr("list")?
        .call1((modules.items(),))?
        .downcast_into::<PyList>()?;
    let entry_prefix = format!("{entry_module}.");
    let mut stale_names = Vec::new();

    for entry in items.iter() {
        let tuple = entry.downcast_into::<PyTuple>()?;
        let Some(key) = tuple.get_item(0).ok() else {
            continue;
        };
        let Some(value) = tuple.get_item(1).ok() else {
            continue;
        };
        let Ok(name) = key.extract::<String>() else {
            continue;
        };
        if (name == entry_module || name.starts_with(entry_prefix.as_str()))
            && !module_matches_search_paths(&value, search_paths)?
        {
            stale_names.push(name);
        }
    }

    for name in stale_names {
        let contains = modules
            .call_method1("__contains__", (name.as_str(),))?
            .is_truthy()?;
        if contains {
            modules.del_item(name.as_str())?;
        }
    }

    Ok(())
}

pub(super) fn import_python_entrypoint_module<'py>(
    py: Python<'py>,
    entry_module: &str,
    search_paths: &[PathBuf],
) -> PyResult<pyo3::Bound<'py, PyModule>> {
    let Some((entry_path, package_dir)) = resolve_entrypoint_path(entry_module, search_paths)
    else {
        return PyModule::import(py, entry_module);
    };

    let importlib_util = py.import("importlib.util")?;
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    let entry_path = entry_path.to_string_lossy().into_owned();
    let spec = if let Some(package_dir) = package_dir {
        let kwargs = PyDict::new(py);
        let locations = PyList::new(py, [package_dir.to_string_lossy().into_owned()])?;
        kwargs.set_item("submodule_search_locations", locations)?;
        importlib_util.call_method(
            "spec_from_file_location",
            (entry_module, entry_path.as_str()),
            Some(&kwargs),
        )?
    } else {
        importlib_util.call_method1(
            "spec_from_file_location",
            (entry_module, entry_path.as_str()),
        )?
    };
    if spec.is_none() {
        return Err(PyRuntimeError::new_err(format!(
            "python entrypoint module '{}' could not be loaded from {}",
            entry_module, entry_path
        )));
    }

    let module = importlib_util.call_method1("module_from_spec", (&spec,))?;
    modules.set_item(entry_module, &module)?;
    let loader = spec.getattr("loader")?;
    if let Err(err) = loader.call_method1("exec_module", (&module,)) {
        let _ = modules.del_item(entry_module);
        return Err(err);
    }
    Ok(module.downcast_into::<PyModule>()?)
}

fn resolve_entrypoint_path(
    entry_module: &str,
    search_paths: &[PathBuf],
) -> Option<(PathBuf, Option<PathBuf>)> {
    let module_relative = entry_module.replace('.', std::path::MAIN_SEPARATOR_STR);
    let file_candidate = format!("{module_relative}.py");
    let package_candidate = PathBuf::from(&module_relative).join("__init__.py");

    for base in search_paths {
        let file_path = base.join(file_candidate.as_str());
        if file_path.exists() {
            return Some((file_path, None));
        }
        let package_path = base.join(&package_candidate);
        if package_path.exists() {
            let package_dir = package_path.parent().map(Path::to_path_buf);
            return Some((package_path, package_dir));
        }
    }

    None
}

fn module_matches_search_paths(
    module: &pyo3::Bound<'_, PyAny>,
    search_paths: &[PathBuf],
) -> PyResult<bool> {
    if let Ok(file_attr) = module.getattr("__file__")
        && let Ok(file_path) = file_attr.extract::<String>()
        && path_matches_search_paths(file_path.as_str(), search_paths)
    {
        return Ok(true);
    }

    if let Ok(path_attr) = module.getattr("__path__") {
        for item in path_attr.try_iter()? {
            let item = item?;
            if let Ok(path) = item.extract::<String>()
                && path_matches_search_paths(path.as_str(), search_paths)
            {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn path_matches_search_paths(raw: &str, search_paths: &[PathBuf]) -> bool {
    let path = Path::new(raw);
    search_paths.iter().any(|base| path.starts_with(base))
}

pub(super) fn remove_python_modules(py: Python<'_>, module_names: &[String]) -> PyResult<()> {
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    for name in module_names {
        let contains = modules
            .call_method1("__contains__", (name.as_str(),))?
            .is_truthy()?;
        if contains {
            modules.del_item(name.as_str())?;
        }
    }
    Ok(())
}

pub(super) fn remove_python_search_paths(py: Python<'_>, search_paths: &[PathBuf]) -> PyResult<()> {
    if search_paths.is_empty() {
        return Ok(());
    }
    let sys = py.import("sys")?;
    let py_path = sys.getattr("path")?;
    for path in search_paths {
        let path_text = path.to_string_lossy().into_owned();
        if path_text.trim().is_empty() {
            continue;
        }
        loop {
            let exists = py_path
                .call_method1("__contains__", (path_text.as_str(),))?
                .is_truthy()?;
            if !exists {
                break;
            }
            py_path.call_method1("remove", (path_text.as_str(),))?;
        }
    }
    Ok(())
}

pub(super) fn py_any_to_json(value: &pyo3::Bound<'_, PyAny>) -> PyResult<Value> {
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

pub(super) fn json_to_pyobject(py: Python<'_>, value: &Value) -> PyResult<PyObject> {
    let json = py.import("json")?;
    let dumped = serde_json::to_string(value)
        .map_err(|err| PyValueError::new_err(format!("json serialization failed: {}", err)))?;
    let parsed = json.call_method1("loads", (dumped,))?;
    Ok(parsed.unbind())
}

pub(super) fn plugin_runtime_error(plugin_id: &str, err: PyErr) -> PluginSdkError {
    PluginSdkError::Runtime(format!(
        "python plugin '{}' load failed: {}",
        plugin_id, err
    ))
}
