use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};

use crate::plugin::sdk::PluginSdkError;
use crate::plugin::sdk::python::bridge_contract::{
    PYTHON_TOOL_ATTR_ACTIVE, PYTHON_TOOL_ATTR_NAME, PYTHON_TOOL_CALL_METHOD,
};

pub(crate) struct PythonToolRegistration<'py> {
    pub(crate) item: Bound<'py, PyAny>,
    pub(crate) name: String,
    pub(crate) active: bool,
}

pub(crate) fn decode_python_tool_registration<'py>(
    plugin_id: &str,
    item: Bound<'py, PyAny>,
) -> Result<PythonToolRegistration<'py>, PluginSdkError> {
    let name = item
        .getattr(PYTHON_TOOL_ATTR_NAME)
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' tool name lookup failed: {}",
                plugin_id, err
            ))
        })?
        .extract::<String>()
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' tool name decode failed: {}",
                plugin_id, err
            ))
        })?;
    let active = item
        .getattr(PYTHON_TOOL_ATTR_ACTIVE)
        .ok()
        .and_then(|value| value.extract::<bool>().ok())
        .unwrap_or(true);
    Ok(PythonToolRegistration { item, name, active })
}

pub(crate) fn invoke_python_tool_registration<'py>(
    registration: &PythonToolRegistration<'py>,
    plugin_id: &str,
    tool_name: &str,
    kwargs: &Bound<'py, PyDict>,
) -> Result<Bound<'py, PyAny>, PluginSdkError> {
    registration
        .item
        .call_method(PYTHON_TOOL_CALL_METHOD, (), Some(kwargs))
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' tool '{}' invocation failed: {}",
                plugin_id, tool_name, err
            ))
        })
}

#[cfg(test)]
#[path = "tool/tests.rs"]
mod tests;
