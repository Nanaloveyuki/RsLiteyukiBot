use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use pyo3::prelude::*;

use crate::adapter::AdapterManager;
use crate::comm::{ChannelRegistry, SharedStore};
use crate::core::{LifecycleContext, RuntimeCapabilities, RuntimeFlavor};
use crate::observability::{Logger, LoggerConfig};
use crate::plugin::sdk::PluginPermissionSet;
use crate::plugin::sdk::host_bridge::PluginHostBridge;
use crate::plugin::sdk::python::bridge::PyPluginSdk;
use crate::plugin::sdk::python::state::PythonRuntimeState;
use crate::session::SessionRouter;

pub(super) fn python_runtime_test_lock() -> MutexGuard<'static, ()> {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("python runtime test lock")
}

pub(super) fn test_plugin_host() -> PluginHostBridge {
    let logger = Logger::with_config(LoggerConfig::default());
    let channels = ChannelRegistry::default();
    let lifecycle = Arc::new(LifecycleContext::new_with_capabilities(
        "test",
        "0.1.0",
        RuntimeFlavor::Cli,
        RuntimeCapabilities {
            cli: true,
            ..RuntimeCapabilities::default()
        },
    ));
    let session_router = SessionRouter::new();
    let shared_store = SharedStore::new(channels.clone());
    PluginHostBridge::new(
        lifecycle,
        channels,
        shared_store,
        session_router,
        AdapterManager::default(),
        logger,
    )
}

pub(super) fn new_test_python_sdk(
    py: Python<'_>,
    state: &Arc<Mutex<PythonRuntimeState>>,
    plugin_id: &str,
) -> Py<PyPluginSdk> {
    Py::new(
        py,
        PyPluginSdk::new(
            plugin_id.to_string(),
            test_plugin_host(),
            state.clone(),
            None,
            PluginPermissionSet::default(),
        ),
    )
    .expect("sdk")
}
