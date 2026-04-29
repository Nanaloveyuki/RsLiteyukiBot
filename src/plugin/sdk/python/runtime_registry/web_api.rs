use pyo3::prelude::*;
use pyo3::types::{PyAny, PyTuple};
use serde::Deserialize;

use crate::plugin::sdk::PluginSdkError;
use crate::plugin::sdk::python::bridge_contract::{
    PYTHON_WEB_API_HANDLER_INDEX, PYTHON_WEB_API_METHODS_INDEX, PYTHON_WEB_API_ROUTE_INDEX,
};

#[derive(Debug, Deserialize)]
pub(crate) struct PythonWebApiSnapshotDoc {
    pub(crate) route: String,
    #[serde(default)]
    pub(crate) methods: Vec<String>,
}

pub(crate) struct PythonWebApiRegistration<'py> {
    pub(crate) route: String,
    pub(crate) methods: Vec<String>,
    pub(crate) handler: Bound<'py, PyAny>,
}

pub(crate) fn decode_python_web_api_registration<'py>(
    plugin_id: &str,
    item: Bound<'py, PyAny>,
) -> Result<PythonWebApiRegistration<'py>, PluginSdkError> {
    let tuple = item.downcast_into::<PyTuple>().map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' web api registration is invalid: {}",
            plugin_id, err
        ))
    })?;
    let document =
        decode_web_api_registration_tuple(plugin_id, &tuple).map_err(PluginSdkError::Runtime)?;
    let handler = tuple
        .get_item(PYTHON_WEB_API_HANDLER_INDEX)
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' web api handler lookup failed: {}",
                plugin_id, err
            ))
        })?;
    Ok(PythonWebApiRegistration {
        route: document.route,
        methods: document.methods,
        handler,
    })
}

fn decode_web_api_registration_tuple(
    plugin_id: &str,
    tuple: &Bound<'_, PyTuple>,
) -> Result<PythonWebApiSnapshotDoc, String> {
    let route = tuple
        .get_item(PYTHON_WEB_API_ROUTE_INDEX)
        .map_err(|err| format!("python plugin '{plugin_id}' web api route lookup failed: {err}"))?
        .extract::<String>()
        .map_err(|err| format!("python plugin '{plugin_id}' web api route decode failed: {err}"))?;
    let methods = tuple
        .get_item(PYTHON_WEB_API_METHODS_INDEX)
        .map_err(|err| format!("python plugin '{plugin_id}' web api methods lookup failed: {err}"))?
        .extract::<Vec<String>>()
        .map_err(|err| format!("python plugin '{plugin_id}' web api methods decode failed: {err}"))?
        .into_iter()
        .map(|value| value.trim().to_ascii_uppercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let route = normalize_python_web_api_route(route.as_str());
    if route.is_empty() || methods.is_empty() {
        return Err(format!(
            "python plugin '{plugin_id}' web api registration is missing route or methods"
        ));
    }
    Ok(PythonWebApiSnapshotDoc { route, methods })
}

fn normalize_python_web_api_route(route: &str) -> String {
    let trimmed = route.trim().trim_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}")
    }
}

#[cfg(test)]
#[path = "web_api/tests.rs"]
mod tests;
