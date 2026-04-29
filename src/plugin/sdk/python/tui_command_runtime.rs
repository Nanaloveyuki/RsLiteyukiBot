use std::sync::{Arc, Mutex};

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyList};

use crate::plugin::sdk::PluginSdkError;
use crate::plugin::sdk::python::bridge::{
    PyPluginSdk, await_python_result, call_python_callable_with_fallback,
    render_python_command_result,
};
use crate::plugin::sdk::python::commands::{is_scope_command_disabled, normalize_tui_command_name};
use crate::plugin::sdk::python::state::PythonRuntimeState;

type PythonCommandHandler = (Py<PyAny>, Py<PyPluginSdk>);

pub(crate) fn execute_python_tui_command(
    state: &Arc<Mutex<PythonRuntimeState>>,
    command: &str,
    args: &[String],
) -> Result<Option<String>, PluginSdkError> {
    let Some(command) = normalize_tui_command_name(command) else {
        return Ok(None);
    };
    let Some((handler, sdk)) = Python::with_gil(
        |py| -> Result<Option<PythonCommandHandler>, PluginSdkError> {
            let lock = state
                .lock()
                .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
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
            callable,
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
