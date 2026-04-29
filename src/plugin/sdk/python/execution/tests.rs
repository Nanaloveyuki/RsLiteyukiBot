use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};

use super::*;
use crate::plugin::sdk::python::bridge::{bind_astrbot_plugin_runtime, install_python_sdk_bridge};
use crate::plugin::sdk::python::bridge_contract::ASTRBOT_RUNTIME_KEY_CRON_JOBS;
use crate::plugin::sdk::python::runtime_registry::load_python_plugin_runtime;
use crate::plugin::sdk::python::state::PythonLoadedPlugin;
use crate::plugin::sdk::python::test_support::{new_test_python_sdk, python_runtime_test_lock};

fn register_test_plugin_with_cron_jobs(
    py: Python<'_>,
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    runtime_module: &str,
    cron_jobs: &Bound<'_, PyList>,
) {
    let sdk = new_test_python_sdk(py, state, plugin_id);
    install_python_sdk_bridge(py, Some(&sdk)).expect("sdk bridge");

    let module = PyModule::new(py, runtime_module).expect("runtime module");
    bind_astrbot_plugin_runtime(py, &module, &sdk).expect("bind runtime");
    let runtime =
        load_python_plugin_runtime(py, plugin_id, runtime_module, "test runtime").expect("runtime");
    runtime
        .set_item(ASTRBOT_RUNTIME_KEY_CRON_JOBS, cron_jobs)
        .expect("cron registry");

    state.lock().expect("runtime lock").plugins.insert(
        plugin_id.to_string(),
        PythonLoadedPlugin {
            event_handler: None,
            start_handler: None,
            health_handler: None,
            shutdown_handler: None,
            unload_handler: None,
            sdk,
            runtime_module: runtime_module.to_string(),
            module_names: vec![runtime_module.to_string()],
            search_paths: Vec::new(),
        },
    );
}

#[test]
// 必要测试
fn execute_python_registered_cron_job_reports_missing_plugin() {
    let _guard = python_runtime_test_lock();
    let state = Arc::new(Mutex::new(PythonRuntimeState::default()));
    let outcome = execute_python_registered_cron_job(&state, "missing", "job_a", &Value::Null)
        .expect("missing plugin should not hard-fail");

    assert!(matches!(
        outcome,
        PythonCronExecutionOutcome::PluginUnavailable
    ));
}

#[test]
// 必要测试
fn execute_python_registered_cron_job_reports_missing_job() {
    let _guard = python_runtime_test_lock();
    let state = Arc::new(Mutex::new(PythonRuntimeState::default()));
    Python::with_gil(|py| {
        let cron_jobs = PyList::empty(py);
        register_test_plugin_with_cron_jobs(
            py,
            &state,
            "demo_missing_job",
            "demo_runtime_missing_job",
            &cron_jobs,
        );
    });

    let outcome =
        execute_python_registered_cron_job(&state, "demo_missing_job", "job_a", &Value::Null)
            .expect("missing cron job should not hard-fail");

    assert!(matches!(outcome, PythonCronExecutionOutcome::JobNotFound));
}

#[test]
// 必要测试
fn execute_python_registered_cron_job_reports_missing_handler() {
    let _guard = python_runtime_test_lock();
    let state = Arc::new(Mutex::new(PythonRuntimeState::default()));
    Python::with_gil(|py| {
        let types = PyModule::import(py, "types").expect("types");
        let kwargs = PyDict::new(py);
        kwargs.set_item("job_id", "job_a").expect("job id");
        kwargs.set_item("handler", py.None()).expect("handler");
        let job = types
            .getattr("SimpleNamespace")
            .expect("simple namespace")
            .call((), Some(&kwargs))
            .expect("job");
        let cron_jobs = PyList::new(py, [job]).expect("cron jobs");
        register_test_plugin_with_cron_jobs(
            py,
            &state,
            "demo_missing_handler",
            "demo_runtime_missing_handler",
            &cron_jobs,
        );
    });

    let outcome =
        execute_python_registered_cron_job(&state, "demo_missing_handler", "job_a", &Value::Null)
            .expect("missing cron handler should not hard-fail");

    assert!(matches!(
        outcome,
        PythonCronExecutionOutcome::HandlerMissing
    ));
}

#[test]
// 必要测试
fn execute_python_registered_cron_job_reports_executed() {
    let _guard = python_runtime_test_lock();
    let state = Arc::new(Mutex::new(PythonRuntimeState::default()));
    Python::with_gil(|py| {
        let builtins = PyModule::import(py, "builtins").expect("builtins");
        let globals = PyDict::new(py);
        builtins
            .getattr("exec")
            .expect("exec")
            .call1((
                "def _cron_handler():\n    return None\n",
                &globals,
                &globals,
            ))
            .expect("define handler");
        let handler = globals
            .get_item("_cron_handler")
            .expect("handler lookup should succeed")
            .expect("handler should exist");
        let types = PyModule::import(py, "types").expect("types");
        let kwargs = PyDict::new(py);
        kwargs.set_item("job_id", "job_a").expect("job id");
        kwargs.set_item("handler", handler).expect("handler");
        let job = types
            .getattr("SimpleNamespace")
            .expect("simple namespace")
            .call((), Some(&kwargs))
            .expect("job");
        let cron_jobs = PyList::new(py, [job]).expect("cron jobs");
        register_test_plugin_with_cron_jobs(
            py,
            &state,
            "demo_executed",
            "demo_runtime_executed",
            &cron_jobs,
        );
    });

    let outcome =
        execute_python_registered_cron_job(&state, "demo_executed", "job_a", &Value::Null)
            .expect("cron job should execute");

    assert!(matches!(outcome, PythonCronExecutionOutcome::Executed));
    let lock = state.lock().expect("runtime lock");
    let diagnostics = lock
        .diagnostics
        .get("demo_executed")
        .expect("diagnostics should exist");
    assert!(diagnostics.last_cron_execution.last_success_at.is_some());
    assert!(diagnostics.last_cron_execution.last_error.is_none());
}
