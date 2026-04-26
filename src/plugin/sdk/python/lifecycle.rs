use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyModule, PyTuple};
use serde::Deserialize;
use serde_json::Value;

use crate::core::BotEvent;
use crate::observability::Logger;
use crate::plugin::sdk::python::bridge::{
    PyPluginSdk, await_python_result, bind_astrbot_plugin_runtime,
    call_python_callable_with_fallback, capture_plugin_module_names,
    cleanup_astrbot_plugin_runtime, import_python_entrypoint_module, install_python_sdk_bridge,
    json_to_pyobject, plugin_runtime_error, py_any_to_json, remove_python_modules,
    remove_python_search_paths, remove_stale_entrypoint_modules, render_python_command_result,
};
use crate::plugin::sdk::python::commands::{
    disabled_declared_command_for_plugin, is_scope_command_disabled, normalize_tui_command_name,
    register_declared_commands,
};
use crate::plugin::sdk::python::probe::{
    PythonCompatibilityProbe, ensure_python_search_paths, inspect_python_legacy_metadata,
    probe_python_plugin_compatibility,
};
use crate::plugin::sdk::{PluginHostBridge, PluginPermissionSet, PluginSdkError};
use crate::plugin::{
    PluginCapabilitySnapshot, PluginDescriptor, PluginExecutionRecord, PluginRegisteredCronJob,
    PluginRegisteredTask, PluginRegisteredTool, PluginRegisteredWebApi, PluginRuntimeDiagnostics,
    PluginToolResult, PluginWebApiRequest, PluginWebApiResponse,
};

const PYTHON_EVENT_HANDLER_ATTRS: [&str; 3] = ["on_event", "handle_event", "liteyuki_handle_event"];
const PYTHON_START_HANDLER_ATTRS: [&str; 3] = ["on_start", "start", "liteyuki_start"];
const PYTHON_HEALTH_HANDLER_ATTRS: [&str; 3] =
    ["on_health_check", "health_check", "liteyuki_health_check"];
const PYTHON_UNLOAD_HANDLER_ATTRS: [&str; 3] = ["on_unload", "unload", "liteyuki_unload"];
const PYTHON_SHUTDOWN_HANDLER_ATTRS: [&str; 3] = ["on_shutdown", "shutdown", "liteyuki_shutdown"];

pub(crate) struct PythonLoadedPlugin {
    pub(crate) event_handler: Option<Py<PyAny>>,
    pub(crate) start_handler: Option<Py<PyAny>>,
    pub(crate) health_handler: Option<Py<PyAny>>,
    pub(crate) shutdown_handler: Option<Py<PyAny>>,
    pub(crate) unload_handler: Option<Py<PyAny>>,
    pub(crate) sdk: Py<PyPluginSdk>,
    pub(crate) runtime_module: String,
    pub(crate) module_names: Vec<String>,
    pub(crate) search_paths: Vec<PathBuf>,
}

pub(crate) struct PythonTuiCommandEntry {
    pub(crate) command: String,
    pub(crate) description: String,
    pub(crate) enabled: bool,
    pub(crate) plugin_id: String,
    pub(crate) handler: Py<PyAny>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ScopedCommandKey {
    pub(crate) scope: String,
    pub(crate) command: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PythonDeclaredCommandEntry {
    pub(crate) command: String,
    pub(crate) description: String,
    pub(crate) plugin_id: String,
    pub(crate) scopes: Vec<String>,
}

#[derive(Default)]
pub(crate) struct PythonRuntimeState {
    pub(crate) plugins: std::collections::HashMap<String, PythonLoadedPlugin>,
    pub(crate) commands: std::collections::HashMap<String, PythonTuiCommandEntry>,
    pub(crate) declared_commands: Vec<PythonDeclaredCommandEntry>,
    pub(crate) disabled_scope_commands: std::collections::HashSet<ScopedCommandKey>,
    pub(crate) diagnostics: std::collections::HashMap<String, PluginRuntimeDiagnostics>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonCapabilitySnapshotDoc {
    #[serde(default)]
    tools: Vec<PluginRegisteredTool>,
    #[serde(default)]
    web_apis: Vec<PluginRegisteredWebApi>,
    #[serde(default)]
    cron_jobs: Vec<PluginRegisteredCronJob>,
    #[serde(default)]
    tasks: Vec<PluginRegisteredTask>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonWebApiSnapshotDoc {
    route: String,
    #[serde(default)]
    methods: Vec<String>,
}

type PythonEventDispatchHandler = (String, Py<PyAny>, Py<PyPluginSdk>);
type PythonCommandHandler = (Py<PyAny>, Py<PyPluginSdk>);

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

pub(crate) fn load_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    descriptor: &PluginDescriptor,
    host: &PluginHostBridge,
    permissions: &PluginPermissionSet,
) -> Result<bool, PluginSdkError> {
    let probe = match probe_python_plugin_compatibility(descriptor) {
        Ok(probe) => probe,
        Err(_) => return Ok(false),
    };
    let plugin_id = descriptor.metadata.id.clone();
    let runtime_state = state.clone();
    let host = host.clone();
    let runtime_options = descriptor.runtime.options.clone();
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
        remove_stale_entrypoint_modules(
            py,
            probe.entrypoint.module.as_str(),
            probe.search_paths.as_slice(),
        )?;

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
        let module = import_python_entrypoint_module(
            py,
            probe.entrypoint.module.as_str(),
            probe.search_paths.as_slice(),
        )?;
        inspect_python_legacy_metadata(&module);

        {
            let mut lock = runtime_state
                .lock()
                .map_err(|_| PyRuntimeError::new_err("python runtime lock poisoned"))?;
            remove_plugin_runtime_state(&mut lock, plugin_id.as_str());
        }

        validate_python_entrypoint_callable(&module, &probe)?;
        invoke_python_bootstrap(py, &module, &probe, &sdk)?;
        let _ = bind_astrbot_plugin_runtime(py, &module, &sdk)?;
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
        let module_names = capture_plugin_module_names(
            py,
            probe.entrypoint.module.as_str(),
            probe.search_paths.as_slice(),
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
                runtime_module: probe.entrypoint.module.clone(),
                module_names,
                search_paths: probe.search_paths.clone(),
            },
        );
        Ok(())
    })
    .map_err(|err| plugin_runtime_error(plugin_id.as_str(), err))?;

    Ok(true)
}

pub(crate) fn get_python_plugin_capability_snapshot(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<Option<PluginCapabilitySnapshot>, PluginSdkError> {
    let runtime_module = {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(None);
        };
        plugin.runtime_module.clone()
    };

    let payload = Python::with_gil(|py| -> Result<Option<Value>, PluginSdkError> {
        let liteyuki = PyModule::import(py, "liteyuki").map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' capability snapshot import failed: {}",
                plugin_id, err
            ))
        })?;
        let snapshotter = liteyuki
            .getattr("_snapshot_astrbot_plugin_runtime")
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' capability snapshot bridge is unavailable: {}",
                    plugin_id, err
                ))
            })?;
        let result = snapshotter
            .call1((runtime_module.as_str(),))
            .map_err(|err| {
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
    })?;

    let Some(payload) = payload else {
        return Ok(None);
    };
    let mut document: PythonCapabilitySnapshotDoc =
        serde_json::from_value(payload).map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' capability snapshot decode failed: {}",
                plugin_id, err
            ))
        })?;
    for tool in &mut document.tools {
        if tool.plugin_id.is_empty() {
            tool.plugin_id = plugin_id.to_string();
        }
    }
    for web_api in &mut document.web_apis {
        if web_api.plugin_id.is_empty() {
            web_api.plugin_id = plugin_id.to_string();
        }
    }
    for cron_job in &mut document.cron_jobs {
        if cron_job.plugin_id.is_empty() {
            cron_job.plugin_id = plugin_id.to_string();
        }
    }
    for task in &mut document.tasks {
        if task.plugin_id.is_empty() {
            task.plugin_id = plugin_id.to_string();
        }
    }

    Ok(Some(PluginCapabilitySnapshot {
        plugin_id: plugin_id.to_string(),
        runtime_kind: crate::plugin::PluginRuntimeKind::Python,
        tools: document.tools,
        web_apis: document.web_apis,
        cron_jobs: document.cron_jobs,
        tasks: document.tasks,
        updated_at: Utc::now().to_rfc3339(),
    }))
}

pub(crate) fn list_all_python_plugin_capability_snapshots(
    state: &Arc<Mutex<PythonRuntimeState>>,
) -> Result<Vec<PluginCapabilitySnapshot>, PluginSdkError> {
    let mut plugin_ids = {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        lock.plugins.keys().cloned().collect::<Vec<_>>()
    };
    plugin_ids.sort();

    let mut snapshots = Vec::with_capacity(plugin_ids.len());
    for plugin_id in plugin_ids {
        if let Some(snapshot) = get_python_plugin_capability_snapshot(state, plugin_id.as_str())? {
            snapshots.push(snapshot);
        }
    }
    Ok(snapshots)
}

pub(crate) fn get_python_plugin_runtime_diagnostics(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<Option<PluginRuntimeDiagnostics>, PluginSdkError> {
    let lock = state
        .lock()
        .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
    Ok(lock.diagnostics.get(plugin_id).cloned())
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
    let normalized_route = normalize_python_web_api_route(route);
    let response = Python::with_gil(
        |py| -> Result<Option<PluginWebApiResponse>, PluginSdkError> {
            let liteyuki = PyModule::import(py, "liteyuki").map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' runtime import failed: {}",
                    plugin_id, err
                ))
            })?;
            let runtime_getter =
                liteyuki
                    .getattr("_get_astrbot_plugin_runtime")
                    .map_err(|err| {
                        PluginSdkError::Runtime(format!(
                            "python plugin '{}' runtime getter is unavailable: {}",
                            plugin_id, err
                        ))
                    })?;
            let runtime = runtime_getter
                .call1((runtime_module.as_str(),))
                .map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' runtime lookup failed: {}",
                        plugin_id, err
                    ))
                })?;
            let runtime = runtime
                .downcast_into::<pyo3::types::PyDict>()
                .map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' runtime state shape is invalid: {}",
                        plugin_id, err
                    ))
                })?;
            let registrations = runtime
                .get_item("registered_web_apis")
                .map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' runtime web api lookup failed: {}",
                        plugin_id, err
                    ))
                })?
                .ok_or_else(|| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' runtime web api registry is missing",
                        plugin_id
                    ))
                })?
                .downcast_into::<PyList>()
                .map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' runtime web api registry is invalid: {}",
                        plugin_id, err
                    ))
                })?;

            let mut matched_response = None;
            for item in registrations.iter() {
                let tuple = item.downcast_into::<PyTuple>().map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' web api registration is invalid: {}",
                        plugin_id, err
                    ))
                })?;
                let registration = decode_web_api_registration_tuple(plugin_id, &tuple)
                    .map_err(PluginSdkError::Runtime)?;
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

                let handler = tuple.get_item(1).map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' web api handler lookup failed: {}",
                        plugin_id, err
                    ))
                })?;
                let request_payload = json_to_pyobject(py, &request_context).map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' web api request serialization failed: {}",
                        plugin_id, err
                    ))
                })?;
                let handler_invoker =
                    liteyuki
                        .getattr("_invoke_astrbot_web_handler")
                        .map_err(|err| {
                            PluginSdkError::Runtime(format!(
                                "python plugin '{}' web api handler invoker is unavailable: {}",
                                plugin_id, err
                            ))
                        })?;
                let result = handler_invoker
                    .call1((handler, request_payload))
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
        let liteyuki = PyModule::import(py, "liteyuki").map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' tool runtime import failed: {}",
                plugin_id, err
            ))
        })?;
        let runtime_getter = liteyuki
            .getattr("_get_astrbot_plugin_runtime")
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool runtime getter is unavailable: {}",
                    plugin_id, err
                ))
            })?;
        let runtime = runtime_getter
            .call1((runtime_module.as_str(),))
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool runtime lookup failed: {}",
                    plugin_id, err
                ))
            })?;
        let runtime = runtime.downcast_into::<PyDict>().map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' tool runtime state shape is invalid: {}",
                plugin_id, err
            ))
        })?;
        let tools = runtime
            .get_item("llm_tools")
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool registry lookup failed: {}",
                    plugin_id, err
                ))
            })?
            .ok_or_else(|| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool registry is missing",
                    plugin_id
                ))
            })?
            .downcast_into::<PyList>()
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool registry is invalid: {}",
                    plugin_id, err
                ))
            })?;

        for item in tools.iter() {
            let registered_name = item
                .getattr("name")
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
            if registered_name.trim() != normalized_tool_name {
                continue;
            }

            let active = item
                .getattr("active")
                .ok()
                .and_then(|value| value.extract::<bool>().ok())
                .unwrap_or(true);
            if !active {
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
            let result = item.call_method("call", (), Some(kwargs)).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' tool '{}' invocation failed: {}",
                    plugin_id, normalized_tool_name, err
                ))
            })?;
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
) -> Result<bool, PluginSdkError> {
    let runtime_module = {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(false);
        };
        plugin.runtime_module.clone()
    };

    let payload = normalize_tool_arguments(payload)?;
    let response = Python::with_gil(|py| -> Result<bool, PluginSdkError> {
        let liteyuki = PyModule::import(py, "liteyuki").map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' cron runtime import failed: {}",
                plugin_id, err
            ))
        })?;
        let runtime_getter = liteyuki
            .getattr("_get_astrbot_plugin_runtime")
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron runtime getter is unavailable: {}",
                    plugin_id, err
                ))
            })?;
        let runtime = runtime_getter
            .call1((runtime_module.as_str(),))
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron runtime lookup failed: {}",
                    plugin_id, err
                ))
            })?;
        let runtime = runtime.downcast_into::<PyDict>().map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' cron runtime state shape is invalid: {}",
                plugin_id, err
            ))
        })?;
        let cron_jobs = runtime
            .get_item("cron_jobs")
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron registry lookup failed: {}",
                    plugin_id, err
                ))
            })?
            .ok_or_else(|| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron registry is missing",
                    plugin_id
                ))
            })?
            .downcast_into::<PyList>()
            .map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron registry is invalid: {}",
                    plugin_id, err
                ))
            })?;

        for item in cron_jobs.iter() {
            let registered_job_id = item
                .getattr("job_id")
                .map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' cron job id lookup failed: {}",
                        plugin_id, err
                    ))
                })?
                .extract::<String>()
                .map_err(|err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' cron job id decode failed: {}",
                        plugin_id, err
                    ))
                })?;
            if registered_job_id.trim() != job_id.trim() {
                continue;
            }

            let enabled = item
                .getattr("enabled")
                .ok()
                .and_then(|value| value.extract::<bool>().ok())
                .unwrap_or(true);
            if !enabled {
                return Err(PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron job '{}' is disabled",
                    plugin_id, job_id
                )));
            }

            let handler = item.getattr("handler").map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' cron handler lookup failed: {}",
                    plugin_id, err
                ))
            })?;
            if handler.is_none() {
                return Ok(false);
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
                handler.call0()
            } else {
                handler.call((), Some(kwargs))
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
            return Ok(true);
        }

        Ok(false)
    });

    match &response {
        Ok(true) => record_plugin_execution_success(state, plugin_id, PluginExecutionKind::Cron),
        Err(err) => record_plugin_execution_error(
            state,
            plugin_id,
            PluginExecutionKind::Cron,
            err.to_string(),
        ),
        Ok(false) => {}
    }

    response
}

pub(crate) fn start_python_manifest_plugin(
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

pub(crate) fn health_check_python_manifest_plugin(
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

pub(crate) fn shutdown_python_manifest_plugin(
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
            .shutdown_handler
            .as_ref()
            .map(|handler| (handler.clone_ref(py), plugin.sdk.clone_ref(py))))
    })?
    else {
        return Ok(());
    };

    Python::with_gil(|py| -> Result<(), PluginSdkError> {
        invoke_python_lifecycle_handler(py, &handler, &sdk).map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' shutdown hook failed: {}",
                plugin_id, err
            ))
        })
    })
}

pub(crate) fn unload_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<(), PluginSdkError> {
    let snapshot = Python::with_gil(|py| -> Result<_, PluginSdkError> {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(None);
        };
        let removable_paths = plugin
            .search_paths
            .iter()
            .filter(|path| !other_plugins_use_search_path(&lock, plugin_id, path))
            .cloned()
            .collect::<Vec<_>>();
        Ok(Some(PythonUnloadSnapshot {
            handler: plugin
                .unload_handler
                .as_ref()
                .map(|handler| handler.clone_ref(py)),
            sdk: plugin.sdk.clone_ref(py),
            module_names: plugin.module_names.clone(),
            removable_paths,
        }))
    })?;

    let hook_result = match snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.handler.as_ref())
    {
        Some(handler) => Python::with_gil(|py| -> Result<(), PluginSdkError> {
            let snapshot = snapshot.as_ref().expect("snapshot should exist");
            invoke_python_lifecycle_handler(py, handler, &snapshot.sdk).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' unload hook failed: {}",
                    plugin_id, err
                ))
            })
        }),
        None => Ok(()),
    };

    let cleanup_python_result = match snapshot {
        Some(snapshot) => Python::with_gil(|py| -> Result<(), PluginSdkError> {
            cleanup_astrbot_plugin_runtime(py, snapshot.module_names.as_slice()).map_err(
                |err| {
                    PluginSdkError::Runtime(format!(
                        "python plugin '{}' astrbot runtime cleanup failed: {}",
                        plugin_id, err
                    ))
                },
            )?;
            remove_python_modules(py, snapshot.module_names.as_slice()).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' unload module cleanup failed: {}",
                    plugin_id, err
                ))
            })?;
            remove_python_search_paths(py, snapshot.removable_paths.as_slice()).map_err(|err| {
                PluginSdkError::Runtime(format!(
                    "python plugin '{}' unload search-path cleanup failed: {}",
                    plugin_id, err
                ))
            })
        }),
        None => Ok(()),
    };

    let cleanup_state_result = {
        let mut lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        remove_plugin_runtime_state(&mut lock, plugin_id);
        Ok(())
    };

    match (hook_result, cleanup_python_result, cleanup_state_result) {
        (_, _, Err(err)) => Err(err),
        (Err(err), _, Ok(())) => Err(err),
        (Ok(()), Err(err), Ok(())) => Err(err),
        (Ok(()), Ok(()), Ok(())) => Ok(()),
    }
}

fn validate_python_entrypoint_callable(
    module: &pyo3::Bound<'_, PyModule>,
    probe: &PythonCompatibilityProbe,
) -> PyResult<()> {
    if let Some(callable_name) = probe.entrypoint.callable.as_deref() {
        let target = module.getattr(callable_name)?;
        if !target.is_callable() {
            return Err(PyTypeError::new_err(format!(
                "python entrypoint '{}' in module '{}' is not callable",
                callable_name, probe.entrypoint.module
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum PluginExecutionKind {
    WebApi,
    Tool,
    Cron,
}

fn normalize_tool_arguments(arguments: &Value) -> Result<Value, PluginSdkError> {
    match arguments {
        Value::Null => Ok(Value::Object(serde_json::Map::new())),
        Value::Object(_) => Ok(arguments.clone()),
        _ => Err(PluginSdkError::Runtime(
            "plugin tool arguments must be a JSON object".to_string(),
        )),
    }
}

fn normalize_plugin_tool_lookup(plugin_id: &str, tool_name: &str) -> String {
    let trimmed = tool_name.trim();
    let prefix = format!("plugin::{plugin_id}::");
    trimmed
        .strip_prefix(prefix.as_str())
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

fn parse_python_tool_result(value: &pyo3::Bound<'_, PyAny>) -> Result<PluginToolResult, String> {
    if value.is_none() {
        return Ok(PluginToolResult::Json(Value::Null));
    }
    if let Ok(text) = value.extract::<String>() {
        return Ok(PluginToolResult::Text(text));
    }
    let json = py_any_to_json(value).map_err(|err| err.to_string())?;
    Ok(PluginToolResult::Json(json))
}

fn record_plugin_execution_success(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    kind: PluginExecutionKind,
) {
    if let Ok(mut lock) = state.lock() {
        let diagnostics = lock
            .diagnostics
            .entry(plugin_id.to_string())
            .or_insert_with(|| PluginRuntimeDiagnostics {
                plugin_id: plugin_id.to_string(),
                ..PluginRuntimeDiagnostics::default()
            });
        let record = execution_record_mut(diagnostics, kind);
        record.last_success_at = Some(Utc::now().to_rfc3339());
    }
}

fn record_plugin_execution_error(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    kind: PluginExecutionKind,
    error: String,
) {
    if let Ok(mut lock) = state.lock() {
        let diagnostics = lock
            .diagnostics
            .entry(plugin_id.to_string())
            .or_insert_with(|| PluginRuntimeDiagnostics {
                plugin_id: plugin_id.to_string(),
                ..PluginRuntimeDiagnostics::default()
            });
        let record = execution_record_mut(diagnostics, kind);
        record.last_error = Some(error);
        record.last_error_at = Some(Utc::now().to_rfc3339());
    }
}

fn execution_record_mut(
    diagnostics: &mut PluginRuntimeDiagnostics,
    kind: PluginExecutionKind,
) -> &mut PluginExecutionRecord {
    match kind {
        PluginExecutionKind::WebApi => &mut diagnostics.last_web_api_dispatch,
        PluginExecutionKind::Tool => &mut diagnostics.last_tool_execution,
        PluginExecutionKind::Cron => &mut diagnostics.last_cron_execution,
    }
}

fn build_plugin_web_api_request_context(request: &PluginWebApiRequest) -> Value {
    let body = request.body.clone();
    let body_json = serde_json::from_slice::<Value>(body.as_slice()).ok();
    let body_text = String::from_utf8(body.clone()).ok();
    let body_bytes_base64 = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(body)
    };
    serde_json::json!({
        "method": request.method,
        "path": request.path,
        "query": request.query,
        "headers": request.headers,
        "bodyJson": body_json,
        "bodyText": body_text,
        "bodyBytesBase64": body_bytes_base64,
        "peerIp": request.peer_ip,
    })
}

fn decode_web_api_registration_tuple(
    plugin_id: &str,
    tuple: &pyo3::Bound<'_, PyTuple>,
) -> Result<PythonWebApiSnapshotDoc, String> {
    let route = tuple
        .get_item(0)
        .map_err(|err| format!("python plugin '{plugin_id}' web api route lookup failed: {err}"))?
        .extract::<String>()
        .map_err(|err| format!("python plugin '{plugin_id}' web api route decode failed: {err}"))?;
    let methods = tuple
        .get_item(2)
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

fn parse_python_web_api_response(
    value: &pyo3::Bound<'_, PyAny>,
) -> Result<PluginWebApiResponse, String> {
    if value.is_none() {
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: Vec::new(),
        });
    }

    if let Ok(bytes) = value.extract::<Vec<u8>>() {
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "application/octet-stream".to_string(),
            body: bytes,
        });
    }

    if let Ok(text) = value.extract::<String>() {
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: text.into_bytes(),
        });
    }

    let json = py_any_to_json(value).map_err(|err| err.to_string())?;
    decode_json_web_api_response(json)
}

fn decode_json_web_api_response(value: Value) -> Result<PluginWebApiResponse, String> {
    match value {
        Value::Array(items) => decode_array_web_api_response(items),
        Value::Object(mut object) => {
            let status_code = object
                .remove("status")
                .and_then(|value| value.as_u64())
                .map(|value| value as u16)
                .unwrap_or(200);
            let explicit_content_type = object
                .remove("contentType")
                .or_else(|| object.remove("content_type"))
                .and_then(|value| value.as_str().map(ToString::to_string));
            let body_value = object.remove("body").unwrap_or(Value::Object(object));
            let (content_type, body) =
                encode_web_api_body(body_value, explicit_content_type.as_deref())?;
            Ok(PluginWebApiResponse {
                status_code,
                content_type,
                body,
            })
        }
        other => {
            let (content_type, body) = encode_web_api_body(other, None)?;
            Ok(PluginWebApiResponse {
                status_code: 200,
                content_type,
                body,
            })
        }
    }
}

fn decode_array_web_api_response(items: Vec<Value>) -> Result<PluginWebApiResponse, String> {
    let Some(first) = items.first() else {
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "application/json; charset=utf-8".to_string(),
            body: b"[]".to_vec(),
        });
    };
    let Some(status_code) = first.as_u64().map(|value| value as u16) else {
        let body = serde_json::to_vec(&Value::Array(items))
            .map_err(|err| format!("response json serialization failed: {err}"))?;
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "application/json; charset=utf-8".to_string(),
            body,
        });
    };
    let body_value = items.get(1).cloned().unwrap_or(Value::Null);
    let content_type = items
        .get(2)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let (content_type, body) = encode_web_api_body(body_value, content_type)?;
    Ok(PluginWebApiResponse {
        status_code,
        content_type,
        body,
    })
}

fn encode_web_api_body(
    value: Value,
    explicit_content_type: Option<&str>,
) -> Result<(String, Vec<u8>), String> {
    match value {
        Value::Null => Ok((
            explicit_content_type
                .unwrap_or("text/plain; charset=utf-8")
                .to_string(),
            Vec::new(),
        )),
        Value::String(text) => Ok((
            explicit_content_type
                .unwrap_or("text/plain; charset=utf-8")
                .to_string(),
            text.into_bytes(),
        )),
        other => Ok((
            explicit_content_type
                .unwrap_or("application/json; charset=utf-8")
                .to_string(),
            serde_json::to_vec(&other)
                .map_err(|err| format!("response json serialization failed: {err}"))?,
        )),
    }
}

fn invoke_python_bootstrap(
    py: Python<'_>,
    module: &pyo3::Bound<'_, PyModule>,
    probe: &PythonCompatibilityProbe,
    sdk: &Py<PyPluginSdk>,
) -> PyResult<()> {
    let callable = if let Some(callable_name) = probe.entrypoint.callable.as_deref() {
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
        return Ok(Some(handler.unbind()));
    }
    for candidate in PYTHON_EVENT_HANDLER_ATTRS {
        if let Ok(handler) = module.getattr(candidate)
            && handler.is_callable()
        {
            return Ok(Some(handler.unbind()));
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
        return Ok(Some(handler.unbind()));
    }
    for candidate in defaults {
        if let Ok(handler) = module.getattr(candidate)
            && handler.is_callable()
        {
            return Ok(Some(handler.unbind()));
        }
    }
    Ok(None)
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
    state.diagnostics.remove(plugin_id);
    state
        .commands
        .retain(|_, command| command.plugin_id.as_str() != plugin_id);
    state
        .declared_commands
        .retain(|command| command.plugin_id.as_str() != plugin_id);
}

fn other_plugins_use_search_path(
    state: &PythonRuntimeState,
    plugin_id: &str,
    path: &PathBuf,
) -> bool {
    state.plugins.iter().any(|(other_id, plugin)| {
        other_id != plugin_id
            && plugin
                .search_paths
                .iter()
                .any(|candidate| candidate == path)
    })
}

struct PythonUnloadSnapshot {
    handler: Option<Py<PyAny>>,
    sdk: Py<PyPluginSdk>,
    module_names: Vec<String>,
    removable_paths: Vec<PathBuf>,
}
