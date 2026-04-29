use std::sync::{Arc, Mutex};

use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

use crate::plugin::PluginToolResult;
use crate::plugin::sdk::python::bridge::await_python_result;
use crate::plugin::sdk::python::bridge_contract::ASTRBOT_INVOKE_WEB_HANDLER_FN;
use crate::plugin::sdk::python::diagnostics::{
    PluginExecutionKind, record_plugin_execution_error, record_plugin_execution_success,
};
use crate::plugin::sdk::python::execution_codec::{
    build_plugin_web_api_request_context, normalize_lookup_web_api_route,
    normalize_plugin_tool_lookup, normalize_tool_arguments, parse_python_tool_result,
    parse_python_web_api_response,
};
use crate::plugin::sdk::python::json_codec::json_to_pyobject;
use crate::plugin::sdk::python::runtime_registry::{
    decode_python_cron_registration, decode_python_tool_registration,
    decode_python_web_api_registration, invoke_python_tool_registration, load_python_cron_registry,
    load_python_plugin_runtime, load_python_tool_registry, load_python_web_api_registry,
};
use crate::plugin::sdk::python::state::PythonRuntimeState;
use crate::plugin::sdk::{PluginSdkError, PluginWebApiRequest, PluginWebApiResponse};

pub(crate) enum PythonCronExecutionOutcome {
    Executed,
    PluginUnavailable,
    JobNotFound,
    HandlerMissing,
}

pub(crate) fn execute_python_registered_web_api(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    route: &str,
    request: &PluginWebApiRequest,
) -> Result<Option<PluginWebApiResponse>, PluginSdkError> {
    let runtime_module = {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(None);
        };
        plugin.runtime_module.clone()
    };

    let request_context = build_plugin_web_api_request_context(request);
    let normalized_route = normalize_lookup_web_api_route(route);
    let response = Python::with_gil(
        |py| -> Result<Option<PluginWebApiResponse>, PluginSdkError> {
            let liteyuki = pyo3::types::PyModule::import(
                py,
                crate::plugin::sdk::python::bridge_contract::PYTHON_ROOT_MODULE,
            )
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' runtime import failed: {}",
                    plugin_id, err
                ))
            })?;
            let runtime =
                load_python_plugin_runtime(py, plugin_id, runtime_module.as_str(), "runtime")?;
            let registrations = load_python_web_api_registry(&runtime, plugin_id)?;

            let mut matched_response = None;
            for item in registrations.iter() {
                let registration = decode_python_web_api_registration(plugin_id, item)?;
                if registration.route != normalized_route {
                    continue;
                }
                if !registration
                    .methods
                    .iter()
                    .any(|method| method.eq_ignore_ascii_case(request.method.as_str()))
                {
                    continue;
                }

                let request_payload = json_to_pyobject(py, &request_context).map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' web api request serialization failed: {}",
                        plugin_id, err
                    ))
                })?;
                let handler_invoker =
                    liteyuki
                        .getattr(ASTRBOT_INVOKE_WEB_HANDLER_FN)
                        .map_err(|err| {
                            PluginSdkError::Runtime(format!(
                                "python plugin '{}' web api handler invoker is unavailable: {}",
                                plugin_id, err
                            ))
                        })?;
                let result = handler_invoker
                    .call1((registration.handler, request_payload))
                    .map_err(|err| {
                        PluginSdkError::Runtime(format!(
                            "python plugin '{}' web api handler invocation failed: {}",
                            plugin_id, err
                        ))
                    })?;
                let awaited = await_python_result(py, result.unbind()).map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' web api handler await failed: {}",
                        plugin_id, err
                    ))
                });
                let parsed = parse_python_web_api_response(awaited?.bind(py)).map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' web api response decode failed: {}",
                        plugin_id, err
                    ))
                })?;
                if matched_response.replace(parsed).is_some() {
                    return Err(PluginSdkError::Runtime(format!(
                        "python plugin '{}' has conflicting web api registrations for route '{}' and method '{}'",
                        plugin_id, normalized_route, request.method
                    )));
                }
            }

            Ok(matched_response)
        },
    );

    match &response {
        Ok(Some(_)) => {
            record_plugin_execution_success(state, plugin_id, PluginExecutionKind::WebApi)
        }
        Err(err) => record_plugin_execution_error(
            state,
            plugin_id,
            PluginExecutionKind::WebApi,
            err.to_string(),
        ),
        Ok(None) => {}
    }

    response
}

pub(crate) fn execute_python_registered_tool(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    tool_name: &str,
    arguments: &Value,
) -> Result<Option<PluginToolResult>, PluginSdkError> {
    let runtime_module = {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(None);
        };
        plugin.runtime_module.clone()
    };

    let normalized_tool_name = normalize_plugin_tool_lookup(plugin_id, tool_name);
    let arguments = normalize_tool_arguments(arguments)?;
    let response = Python::with_gil(|py| -> Result<Option<PluginToolResult>, PluginSdkError> {
        let runtime =
            load_python_plugin_runtime(py, plugin_id, runtime_module.as_str(), "tool runtime")?;
        let tools = load_python_tool_registry(&runtime, plugin_id)?;

        for item in tools.iter() {
            let registration = decode_python_tool_registration(plugin_id, item)?;
            if registration.name.trim() != normalized_tool_name {
                continue;
            }

            if !registration.active {
                return Err(PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool '{}' is inactive",
                    plugin_id, normalized_tool_name
                )));
            }

            let kwargs_object = json_to_pyobject(py, &arguments).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool argument serialization failed: {}",
                    plugin_id, err
                ))
            })?;
            let kwargs = kwargs_object.bind(py).downcast::<PyDict>().map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool arguments must decode to a python dict: {}",
                    plugin_id, err
                ))
            })?;
            let result = invoke_python_tool_registration(
                &registration,
                plugin_id,
                normalized_tool_name.as_str(),
                kwargs,
            )?;
            let awaited = await_python_result(py, result.unbind()).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool '{}' await failed: {}",
                    plugin_id, normalized_tool_name, err
                ))
            })?;
            let parsed = parse_python_tool_result(awaited.bind(py)).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool '{}' output decode failed: {}",
                    plugin_id, normalized_tool_name, err
                ))
            })?;
            return Ok(Some(parsed));
        }

        Ok(None)
    });

    match &response {
        Ok(Some(_)) => record_plugin_execution_success(state, plugin_id, PluginExecutionKind::Tool),
        Err(err) => record_plugin_execution_error(
            state,
            plugin_id,
            PluginExecutionKind::Tool,
            err.to_string(),
        ),
        Ok(None) => {}
    }

    response
}

pub(crate) fn execute_python_registered_cron_job(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    job_id: &str,
    payload: &Value,
) -> Result<PythonCronExecutionOutcome, PluginSdkError> {
    let runtime_module = {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(PythonCronExecutionOutcome::PluginUnavailable);
        };
        plugin.runtime_module.clone()
    };

    let payload = normalize_tool_arguments(payload)?;
    let response = Python::with_gil(|py| -> Result<PythonCronExecutionOutcome, PluginSdkError> {
        let runtime =
            load_python_plugin_runtime(py, plugin_id, runtime_module.as_str(), "cron runtime")?;
        let cron_jobs = load_python_cron_registry(&runtime, plugin_id)?;

        for item in cron_jobs.iter() {
            let registration = decode_python_cron_registration(plugin_id, item)?;
            if registration.job_id.trim() != job_id.trim() {
                continue;
            }

            if !registration.enabled {
                return Err(PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron job '{}' is disabled",
                    plugin_id, job_id
                )));
            }

            if registration.handler.is_none() {
                return Ok(PythonCronExecutionOutcome::HandlerMissing);
            }

            let kwargs_object = json_to_pyobject(py, &payload).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron payload serialization failed: {}",
                    plugin_id, err
                ))
            })?;
            let kwargs = kwargs_object.bind(py).downcast::<PyDict>().map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron payload must decode to a python dict: {}",
                    plugin_id, err
                ))
            })?;
            let result = if kwargs.is_empty() {
                registration.handler.call0()
            } else {
                registration.handler.call((), Some(kwargs))
            }
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron job '{}' invocation failed: {}",
                    plugin_id, job_id, err
                ))
            })?;
            let _ = await_python_result(py, result.unbind()).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron job '{}' await failed: {}",
                    plugin_id, job_id, err
                ))
            })?;
            return Ok(PythonCronExecutionOutcome::Executed);
        }

        Ok(PythonCronExecutionOutcome::JobNotFound)
    });

    match &response {
        Ok(PythonCronExecutionOutcome::Executed) => {
            record_plugin_execution_success(state, plugin_id, PluginExecutionKind::Cron)
        }
        Err(err) => record_plugin_execution_error(
            state,
            plugin_id,
            PluginExecutionKind::Cron,
            err.to_string(),
        ),
        Ok(
            PythonCronExecutionOutcome::PluginUnavailable
            | PythonCronExecutionOutcome::JobNotFound
            | PythonCronExecutionOutcome::HandlerMissing,
        ) => {}
    }

    response
}
#[cfg(test)]
#[path = "execution/tests.rs"]
mod tests;
