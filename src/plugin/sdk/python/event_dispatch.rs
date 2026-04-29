use std::sync::{Arc, Mutex};

use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::core::BotEvent;
use crate::observability::Logger;
use crate::plugin::sdk::python::bridge::{
    PyPluginSdk, await_python_result, call_python_callable_with_fallback,
};
use crate::plugin::sdk::python::commands::disabled_declared_command_for_plugin;
use crate::plugin::sdk::python::json_codec::json_to_pyobject;
use crate::plugin::sdk::python::state::PythonRuntimeState;

type PythonEventDispatchHandler = (String, Py<PyAny>, Py<PyPluginSdk>);

pub(crate) fn dispatch_python_event(
    state: &Arc<Mutex<PythonRuntimeState>>,
    event: &BotEvent,
    logger: &Logger,
) {
    let handlers: Vec<PythonEventDispatchHandler> =
        match Python::with_gil(|py| -> Result<Vec<PythonEventDispatchHandler>, String> {
            let lock = state
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
        }) {
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
                callable,
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
