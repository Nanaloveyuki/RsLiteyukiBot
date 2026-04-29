use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};
use serde_json::Value;

use crate::plugin::sdk::PluginSdkError;
use crate::plugin::sdk::python::bridge_contract::{
    ASTRBOT_GET_RUNTIME_FN, ASTRBOT_RUNTIME_KEY_CRON_JOBS, ASTRBOT_RUNTIME_KEY_REGISTERED_WEB_APIS,
    ASTRBOT_RUNTIME_KEY_TOOLS, ASTRBOT_SNAPSHOT_RUNTIME_FN, PYTHON_ROOT_MODULE,
};
use crate::plugin::sdk::python::json_codec::py_any_to_json;

pub(crate) fn load_python_plugin_runtime<'py>(
    py: Python<'py>,
    plugin_id: &str,
    runtime_module: &str,
    context: &str,
) -> Result<Bound<'py, PyDict>, PluginSdkError> {
    let liteyuki = PyModule::import(py, PYTHON_ROOT_MODULE).map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' {} import failed: {}",
            plugin_id, context, err
        ))
    })?;
    let runtime_getter = liteyuki.getattr(ASTRBOT_GET_RUNTIME_FN).map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' {} getter is unavailable: {}",
            plugin_id, context, err
        ))
    })?;
    let runtime = runtime_getter.call1((runtime_module,)).map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' {} lookup failed: {}",
            plugin_id, context, err
        ))
    })?;
    runtime.downcast_into::<PyDict>().map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' {} state shape is invalid: {}",
            plugin_id, context, err
        ))
    })
}

pub(crate) fn load_python_runtime_registry<'py>(
    runtime: &Bound<'py, PyDict>,
    plugin_id: &str,
    key: &'static str,
    label: &str,
) -> Result<Bound<'py, PyList>, PluginSdkError> {
    runtime
        .get_item(key)
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' {} lookup failed: {}",
                plugin_id, label, err
            ))
        })?
        .ok_or_else(|| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' {} is missing",
                plugin_id, label
            ))
        })?
        .downcast_into::<PyList>()
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' {} is invalid: {}",
                plugin_id, label, err
            ))
        })
}

pub(crate) fn load_python_web_api_registry<'py>(
    runtime: &Bound<'py, PyDict>,
    plugin_id: &str,
) -> Result<Bound<'py, PyList>, PluginSdkError> {
    load_python_runtime_registry(
        runtime,
        plugin_id,
        ASTRBOT_RUNTIME_KEY_REGISTERED_WEB_APIS,
        "runtime web api registry",
    )
}

pub(crate) fn load_python_tool_registry<'py>(
    runtime: &Bound<'py, PyDict>,
    plugin_id: &str,
) -> Result<Bound<'py, PyList>, PluginSdkError> {
    load_python_runtime_registry(
        runtime,
        plugin_id,
        ASTRBOT_RUNTIME_KEY_TOOLS,
        "tool registry",
    )
}

pub(crate) fn load_python_cron_registry<'py>(
    runtime: &Bound<'py, PyDict>,
    plugin_id: &str,
) -> Result<Bound<'py, PyList>, PluginSdkError> {
    load_python_runtime_registry(
        runtime,
        plugin_id,
        ASTRBOT_RUNTIME_KEY_CRON_JOBS,
        "cron registry",
    )
}

pub(crate) fn fetch_python_plugin_capability_snapshot_payload(
    py: Python<'_>,
    plugin_id: &str,
    runtime_module: &str,
) -> Result<Option<Value>, PluginSdkError> {
    let liteyuki = PyModule::import(py, PYTHON_ROOT_MODULE).map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' capability snapshot import failed: {}",
            plugin_id, err
        ))
    })?;
    let snapshotter = liteyuki
        .getattr(ASTRBOT_SNAPSHOT_RUNTIME_FN)
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' capability snapshot bridge is unavailable: {}",
                plugin_id, err
            ))
        })?;
    let result = snapshotter.call1((runtime_module,)).map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' capability snapshot call failed: {}",
            plugin_id, err
        ))
    })?;
    if result.is_none() {
        return Ok(None);
    }
    py_any_to_json(&result).map(Some).map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' capability snapshot serialization failed: {}",
            plugin_id, err
        ))
    })
}

#[cfg(test)]
#[path = "common/tests.rs"]
mod tests;
