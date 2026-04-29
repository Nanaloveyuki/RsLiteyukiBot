use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::plugin::sdk::PluginSdkError;
use crate::plugin::sdk::python::bridge::{
    PyPluginSdk, await_python_result, call_python_callable_with_fallback,
    cleanup_astrbot_plugin_runtime,
};
use crate::plugin::sdk::python::module_management::{
    remove_python_modules, remove_python_search_paths,
};
use crate::plugin::sdk::python::state::{
    PythonRuntimeState, other_plugins_use_search_path, remove_plugin_runtime_state,
};

pub(crate) fn start_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<(), PluginSdkError> {
    invoke_manifest_lifecycle_hook(state, plugin_id, "start hook", |py, plugin| {
        Ok(plugin
            .start_handler
            .as_ref()
            .map(|handler| (handler.clone_ref(py), plugin.sdk.clone_ref(py))))
    })
}

pub(crate) fn health_check_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<(), PluginSdkError> {
    invoke_manifest_lifecycle_hook(state, plugin_id, "health check", |py, plugin| {
        Ok(plugin
            .health_handler
            .as_ref()
            .map(|handler| (handler.clone_ref(py), plugin.sdk.clone_ref(py))))
    })
}

pub(crate) fn shutdown_python_manifest_plugin(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Result<(), PluginSdkError> {
    invoke_manifest_lifecycle_hook(state, plugin_id, "shutdown hook", |py, plugin| {
        Ok(plugin
            .shutdown_handler
            .as_ref()
            .map(|handler| (handler.clone_ref(py), plugin.sdk.clone_ref(py))))
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

fn invoke_manifest_lifecycle_hook<F>(
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    action: &str,
    selector: F,
) -> Result<(), PluginSdkError>
where
    F: for<'py> FnOnce(
        Python<'py>,
        &crate::plugin::sdk::python::state::PythonLoadedPlugin,
    ) -> Result<Option<(Py<PyAny>, Py<PyPluginSdk>)>, PluginSdkError>,
{
    let Some((handler, sdk)) = Python::with_gil(|py| -> Result<_, PluginSdkError> {
        let lock = state
            .lock()
            .map_err(|_| PluginSdkError::Runtime("python runtime lock poisoned".to_string()))?;
        let Some(plugin) = lock.plugins.get(plugin_id) else {
            return Ok(None);
        };
        selector(py, plugin)
    })?
    else {
        return Ok(());
    };

    Python::with_gil(|py| -> Result<(), PluginSdkError> {
        invoke_python_lifecycle_handler(py, &handler, &sdk).map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' {} failed: {}",
                plugin_id, action, err
            ))
        })
    })
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

struct PythonUnloadSnapshot {
    handler: Option<Py<PyAny>>,
    sdk: Py<PyPluginSdk>,
    module_names: Vec<String>,
    removable_paths: Vec<PathBuf>,
}
