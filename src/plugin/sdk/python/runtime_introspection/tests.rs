use std::sync::{Arc, Mutex};

use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods, PyModule, PyModuleMethods};
use serde_json::json;

use super::*;
use crate::plugin::PluginExecutionRecord;
use crate::plugin::sdk::python::json_codec::json_to_pyobject;
use crate::plugin::sdk::python::state::{PythonLoadedPlugin, PythonRuntimeState};
use crate::plugin::sdk::python::test_support::{new_test_python_sdk, python_runtime_test_lock};

#[test]
// 必要测试
fn populate_snapshot_plugin_ids_backfills_missing_entries() {
    let mut document = PythonCapabilitySnapshotDoc {
        tools: vec![PluginRegisteredTool::default()],
        web_apis: vec![PluginRegisteredWebApi::default()],
        cron_jobs: vec![PluginRegisteredCronJob::default()],
        tasks: vec![PluginRegisteredTask::default()],
    };

    populate_snapshot_plugin_ids(&mut document, "demo");

    assert_eq!(document.tools[0].plugin_id, "demo");
    assert_eq!(document.web_apis[0].plugin_id, "demo");
    assert_eq!(document.cron_jobs[0].plugin_id, "demo");
    assert_eq!(document.tasks[0].plugin_id, "demo");
}

#[test]
// 必要测试
fn get_python_plugin_capability_snapshot_decodes_payload_and_backfills_plugin_ids() {
    let _guard = python_runtime_test_lock();
    let state = Arc::new(Mutex::new(PythonRuntimeState::default()));
    Python::with_gil(|py| {
        install_fake_snapshot_module(
            py,
            &json!({
                "demo_runtime": {
                    "tools": [{
                        "pluginId": "",
                        "name": "tool_a",
                        "description": "demo tool",
                        "parameters": {},
                        "active": true
                    }],
                    "webApis": [{
                        "pluginId": "",
                        "route": "/demo",
                        "methods": ["GET"]
                    }],
                    "cronJobs": [{
                        "pluginId": "",
                        "jobId": "job_a",
                        "enabled": true
                    }],
                    "tasks": [{
                        "pluginId": "",
                        "taskId": "task_a",
                        "taskKind": "manual"
                    }]
                }
            }),
        );
        register_test_loaded_plugin(py, &state, "demo", "demo_runtime");
    });

    let snapshot = get_python_plugin_capability_snapshot(&state, "demo")
        .expect("snapshot fetch should succeed")
        .expect("snapshot should exist");

    assert_eq!(snapshot.plugin_id, "demo");
    assert_eq!(snapshot.tools[0].plugin_id, "demo");
    assert_eq!(snapshot.web_apis[0].plugin_id, "demo");
    assert_eq!(snapshot.cron_jobs[0].plugin_id, "demo");
    assert_eq!(snapshot.tasks[0].plugin_id, "demo");
    assert!(!snapshot.updated_at.is_empty());
}

#[test]
// 必要测试
fn list_all_python_plugin_capability_snapshots_sorts_ids_and_skips_none() {
    let _guard = python_runtime_test_lock();
    let state = Arc::new(Mutex::new(PythonRuntimeState::default()));
    Python::with_gil(|py| {
        install_fake_snapshot_module(
            py,
            &json!({
                "alpha_runtime": { "tools": [{ "name": "tool_alpha" }] },
                "beta_runtime": { "tools": [{ "name": "tool_beta" }] },
                "gamma_runtime": null
            }),
        );
        register_test_loaded_plugin(py, &state, "beta", "beta_runtime");
        register_test_loaded_plugin(py, &state, "alpha", "alpha_runtime");
        register_test_loaded_plugin(py, &state, "gamma", "gamma_runtime");
    });

    let snapshots = list_all_python_plugin_capability_snapshots(&state)
        .expect("listing snapshots should succeed");
    let plugin_ids = snapshots
        .iter()
        .map(|snapshot| snapshot.plugin_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(plugin_ids, vec!["alpha", "beta"]);
}

#[test]
// 必要测试
fn get_python_plugin_capability_snapshot_reports_decode_error() {
    let _guard = python_runtime_test_lock();
    let state = Arc::new(Mutex::new(PythonRuntimeState::default()));
    Python::with_gil(|py| {
        install_fake_snapshot_module(py, &json!({ "broken_runtime": { "tools": 42 } }));
        register_test_loaded_plugin(py, &state, "broken", "broken_runtime");
    });

    let error = get_python_plugin_capability_snapshot(&state, "broken")
        .expect_err("invalid snapshot payload should fail");

    assert!(
        error
            .to_string()
            .contains("capability snapshot decode failed")
    );
}

#[test]
// 必要测试
fn get_python_plugin_runtime_diagnostics_returns_cloned_snapshot() {
    let state = Arc::new(Mutex::new(PythonRuntimeState::default()));
    {
        let mut lock = state.lock().expect("runtime lock");
        lock.diagnostics.insert(
            "demo".to_string(),
            PluginRuntimeDiagnostics {
                plugin_id: "demo".to_string(),
                last_tool_execution: PluginExecutionRecord {
                    last_error: Some("tool failed".to_string()),
                    ..PluginExecutionRecord::default()
                },
                ..PluginRuntimeDiagnostics::default()
            },
        );
    }

    let mut diagnostics = get_python_plugin_runtime_diagnostics(&state, "demo")
        .expect("diagnostics fetch should succeed")
        .expect("diagnostics should exist");
    diagnostics.plugin_id = "mutated".to_string();
    diagnostics.last_tool_execution.last_error = Some("mutated".to_string());

    let stored = get_python_plugin_runtime_diagnostics(&state, "demo")
        .expect("diagnostics fetch should succeed")
        .expect("diagnostics should still exist");
    assert_eq!(stored.plugin_id, "demo");
    assert_eq!(
        stored.last_tool_execution.last_error.as_deref(),
        Some("tool failed")
    );
}

fn install_fake_snapshot_module(py: Python<'_>, payloads: &serde_json::Value) {
    let sys = py.import("sys").expect("sys should import");
    let modules = sys
        .getattr("modules")
        .expect("sys.modules should exist")
        .downcast_into::<PyDict>()
        .expect("sys.modules should be a dict");
    let module = PyModule::new(py, "liteyuki").expect("liteyuki module");
    let module_dict = module.dict();
    module
        .setattr(
            "_runtime_payloads",
            json_to_pyobject(py, payloads).expect("payload conversion"),
        )
        .expect("runtime payloads");
    py.import("builtins")
        .expect("builtins should import")
        .getattr("exec")
        .expect("exec should exist")
        .call1((
            "def _snapshot_astrbot_plugin_runtime(module_name):\n    return _runtime_payloads.get(module_name)\n",
            &module_dict,
            &module_dict,
        ))
        .expect("define snapshot function");
    modules
        .set_item("liteyuki", &module)
        .expect("register liteyuki module");
}

fn register_test_loaded_plugin(
    py: Python<'_>,
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
    runtime_module: &str,
) {
    let sdk = new_test_python_sdk(py, state, plugin_id);
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
