use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use liteyukibot_core::{
    ChannelRegistry, LifecycleContext, Plugin, PluginContext, PluginManager, PluginMetadata,
    PluginSdk, PluginType, RuntimeTarget, SessionRouter, SharedStore, PluginHostBridge,
    PluginLoadState,
};
use liteyukibot_core::{Logger, LoggerConfig};
use liteyukibot_core::{RuntimeCapabilities, RuntimeFlavor};
use tokio::time::{Duration, timeout};

struct CountingPlugin {
    id: String,
    name: String,
    loaded: Arc<AtomicUsize>,
}

impl Plugin for CountingPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: self.id.clone(),
            name: self.name.clone(),
            description: "counting".to_string(),
            plugin_type: PluginType::Service,
            author: "".to_string(),
            homepage: "".to_string(),
            extra: HashMap::new(),
        }
    }

    fn on_load(
        &self,
        _context: PluginContext,
    ) -> liteyukibot_core::PluginFuture {
        let loaded = Arc::clone(&self.loaded);
        Box::pin(async move {
            loaded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

fn plugin_context() -> PluginContext {
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
    let host = PluginHostBridge::new(
        lifecycle.clone(),
        channels.clone(),
        shared_store.clone(),
        session_router.clone(),
        logger.clone(),
    );
    PluginContext {
        target: RuntimeTarget::Cli,
        lifecycle,
        channels: channels.clone(),
        shared_store,
        session_router,
        logger,
        sdk: PluginSdk::default(),
        host,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_register_and_load_plugin() {
    let manager = PluginManager::new();
    let loaded = Arc::new(AtomicUsize::new(0));

    manager
        .register_plugin(CountingPlugin {
            id: "counting".to_string(),
            name: "Counting Plugin".to_string(),
            loaded: Arc::clone(&loaded),
        })
        .expect("register should succeed");

    timeout(
        Duration::from_secs(1),
        manager.load_plugin("counting", plugin_context()),
    )
    .await
    .expect("load should not timeout")
    .expect("load should succeed");

    assert!(manager.is_loaded("counting"));
    assert_eq!(loaded.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_discovers_manifest_plugins() {
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("echo_plugin");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "echo-plugin",
  "name": "Echo Plugin",
  "description": "manifest test",
  "type": "service"
}"#,
    )
    .expect("manifest should be written");

    let discovered = manager
        .discover_manifest_plugins_in_dirs([dir.path.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["echo-plugin".to_string()]);

    manager
        .load_plugins(discovered, plugin_context())
        .await
        .expect("manifest plugin should load");

    assert!(manager.is_loaded("echo-plugin"));
    assert_eq!(manager.loaded_plugins().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_marks_python_runtime_as_deferred_plan() {
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "python-echo",
  "name": "Python Echo",
  "type": "service",
  "runtime": {
    "kind": "python",
    "entrypoint": "echo:main"
  }
}"#,
    )
    .expect("manifest should be written");

    let discovered = manager
        .discover_manifest_plugins_in_dirs([dir.path.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-echo".to_string()]);

    manager
        .load_plugins(discovered, plugin_context())
        .await
        .expect("python descriptor plugin should load in deferred mode");

    let loaded = manager.loaded_plugins();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].descriptor.metadata.id, "python-echo");
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Deferred);
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn create() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("liteyuki-rs-plugin-test-{nanos}"));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = remove_dir_all_safe(&self.path);
    }
}

fn remove_dir_all_safe(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}
