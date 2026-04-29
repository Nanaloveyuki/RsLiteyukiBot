use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyModule};
use serde_json::Value;

use crate::plugin::PluginDescriptor;
use crate::plugin::sdk::python::bridge::{
    PyPluginSdk, await_python_result, bind_astrbot_plugin_runtime,
    call_python_callable_with_fallback, install_python_sdk_bridge, plugin_runtime_error,
};
use crate::plugin::sdk::python::bridge_contract::{
    PYTHON_BOOTSTRAP_HANDLER_ATTR, PYTHON_EVENT_HANDLER_ATTRS, PYTHON_HEALTH_HANDLER_ATTRS,
    PYTHON_RUNTIME_OPTION_CONFIG_PATH, PYTHON_RUNTIME_OPTION_EVENT_HANDLER,
    PYTHON_RUNTIME_OPTION_HEALTH_HANDLER, PYTHON_RUNTIME_OPTION_SHUTDOWN_HANDLER,
    PYTHON_RUNTIME_OPTION_START_HANDLER, PYTHON_RUNTIME_OPTION_UNLOAD_HANDLER,
    PYTHON_SHUTDOWN_HANDLER_ATTRS, PYTHON_START_HANDLER_ATTRS, PYTHON_UNLOAD_HANDLER_ATTRS,
};
use crate::plugin::sdk::python::commands::register_declared_commands;
use crate::plugin::sdk::python::module_management::{
    capture_plugin_module_names, import_python_entrypoint_module, remove_stale_entrypoint_modules,
};
use crate::plugin::sdk::python::probe::{
    PythonCompatibilityProbe, ensure_python_search_paths, inspect_python_legacy_metadata,
    probe_python_plugin_compatibility,
};
use crate::plugin::sdk::python::state::{
    PythonLoadedPlugin, PythonRuntimeState, remove_plugin_runtime_state,
};
use crate::plugin::sdk::{PluginHostBridge, PluginPermissionSet, PluginSdkError};

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
        .get(PYTHON_RUNTIME_OPTION_EVENT_HANDLER)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(ToString::to_string);
    let start_handler_override = runtime_options
        .get(PYTHON_RUNTIME_OPTION_START_HANDLER)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(ToString::to_string);
    let health_handler_override = runtime_options
        .get(PYTHON_RUNTIME_OPTION_HEALTH_HANDLER)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(ToString::to_string);
    let shutdown_handler_override = runtime_options
        .get(PYTHON_RUNTIME_OPTION_SHUTDOWN_HANDLER)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(ToString::to_string);
    let unload_handler_override = runtime_options
        .get(PYTHON_RUNTIME_OPTION_UNLOAD_HANDLER)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(ToString::to_string);
    let config_path = runtime_options
        .get(PYTHON_RUNTIME_OPTION_CONFIG_PATH)
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

fn validate_python_entrypoint_callable(
    module: &Bound<'_, PyModule>,
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

fn invoke_python_bootstrap(
    py: Python<'_>,
    module: &Bound<'_, PyModule>,
    probe: &PythonCompatibilityProbe,
    sdk: &Py<PyPluginSdk>,
) -> PyResult<()> {
    let callable = if let Some(callable_name) = probe.entrypoint.callable.as_deref() {
        Some(module.getattr(callable_name)?)
    } else if let Ok(attr) = module.getattr(PYTHON_BOOTSTRAP_HANDLER_ATTR) {
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
    module: &Bound<'_, PyModule>,
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
    module: &Bound<'_, PyModule>,
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
