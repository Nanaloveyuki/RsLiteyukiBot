use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use liteyukibot_core::{
    AdapterConfig, AdapterEndpoint, AdapterManager, AdapterTransport, ChannelRegistry,
    LifecycleContext, Plugin, PluginAbiMethod, PluginContext, PluginHostBridge, PluginLoadError,
    PluginLoadState, PluginManager, PluginManifestLoader, PluginMetadata, PluginRuntimeKind,
    PluginSdk, PluginType, RuntimeTarget, SessionRouter, SharedStore, recent_buffered_logs,
};
use liteyukibot_core::{BotEvent, Logger, LoggerConfig};
use liteyukibot_core::{RuntimeCapabilities, RuntimeFlavor};
use pyo3::prelude::*;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::time::{Duration, sleep, timeout};

struct CountingPlugin {
    id: String,
    name: String,
    loaded: Arc<AtomicUsize>,
}

struct NativeLifecyclePlugin {
    id: String,
    name: String,
    starts: Arc<AtomicUsize>,
    health_checks: Arc<AtomicUsize>,
    shutdowns: Arc<AtomicUsize>,
    unloads: Arc<AtomicUsize>,
}

struct BlockingPlugin {
    id: String,
    name: String,
    loaded: Arc<AtomicUsize>,
    release: Arc<Notify>,
}

impl Plugin for BlockingPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: self.id.clone(),
            name: self.name.clone(),
            description: "blocking".to_string(),
            plugin_type: PluginType::Service,
            author: "".to_string(),
            homepage: "".to_string(),
            extra: HashMap::new(),
        }
    }

    fn on_load(&self, _context: PluginContext) -> liteyukibot_core::PluginFuture {
        let loaded = Arc::clone(&self.loaded);
        let release = Arc::clone(&self.release);
        Box::pin(async move {
            loaded.fetch_add(1, Ordering::SeqCst);
            release.notified().await;
            Ok(())
        })
    }
}

struct FailingPlugin {
    id: String,
    name: String,
    attempts: Arc<AtomicUsize>,
    reason: String,
}

impl Plugin for FailingPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: self.id.clone(),
            name: self.name.clone(),
            description: "failing".to_string(),
            plugin_type: PluginType::Service,
            author: "".to_string(),
            homepage: "".to_string(),
            extra: HashMap::new(),
        }
    }

    fn on_load(&self, _context: PluginContext) -> liteyukibot_core::PluginFuture {
        let attempts = Arc::clone(&self.attempts);
        let reason = self.reason.clone();
        Box::pin(async move {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err(reason)
        })
    }
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

    fn on_load(&self, _context: PluginContext) -> liteyukibot_core::PluginFuture {
        let loaded = Arc::clone(&self.loaded);
        Box::pin(async move {
            loaded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

impl Plugin for NativeLifecyclePlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: self.id.clone(),
            name: self.name.clone(),
            description: "native lifecycle".to_string(),
            plugin_type: PluginType::Service,
            author: "".to_string(),
            homepage: "".to_string(),
            extra: HashMap::new(),
        }
    }

    fn on_load(&self, _context: PluginContext) -> liteyukibot_core::PluginFuture {
        Box::pin(async { Ok(()) })
    }

    fn on_start(&self, _context: PluginContext) -> liteyukibot_core::PluginFuture {
        let starts = Arc::clone(&self.starts);
        Box::pin(async move {
            starts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn on_health_check(&self, _context: PluginContext) -> liteyukibot_core::PluginFuture {
        let health_checks = Arc::clone(&self.health_checks);
        Box::pin(async move {
            health_checks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn on_shutdown(&self, _context: PluginContext) -> liteyukibot_core::PluginFuture {
        let shutdowns = Arc::clone(&self.shutdowns);
        Box::pin(async move {
            shutdowns.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn on_unload(&self, _context: PluginContext) -> liteyukibot_core::PluginFuture {
        let unloads = Arc::clone(&self.unloads);
        Box::pin(async move {
            unloads.fetch_add(1, Ordering::SeqCst);
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
        AdapterManager::default(),
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
    let loaded_plugins = manager.loaded_plugins();
    assert_eq!(loaded_plugins.len(), 1);
    assert_eq!(
        loaded_plugins[0].load_plan.runtime_kind,
        PluginRuntimeKind::Native
    );
    assert_eq!(loaded_plugins[0].load_plan.state, PluginLoadState::Deferred);
    assert_eq!(
        loaded_plugins[0].load_plan.contract.abi_name,
        "liteyuki-native"
    );
    assert!(
        loaded_plugins[0]
            .load_plan
            .contract
            .required_methods
            .contains(&PluginAbiMethod::HandleEvent)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_runs_lifecycle_hooks_for_native_deferred_plugins() {
    let manager = PluginManager::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let health_checks = Arc::new(AtomicUsize::new(0));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let unloads = Arc::new(AtomicUsize::new(0));
    let context = plugin_context();

    manager
        .register_plugin(NativeLifecyclePlugin {
            id: "native-lifecycle".to_string(),
            name: "Native Lifecycle Plugin".to_string(),
            starts: Arc::clone(&starts),
            health_checks: Arc::clone(&health_checks),
            shutdowns: Arc::clone(&shutdowns),
            unloads: Arc::clone(&unloads),
        })
        .expect("register should succeed");

    let loaded = manager
        .load_plugin("native-lifecycle", context.clone())
        .await
        .expect("load should succeed");
    assert_eq!(loaded.load_plan.state, PluginLoadState::Deferred);

    manager
        .start_loaded_plugins(context.clone())
        .await
        .expect("native deferred start should still run");
    manager
        .health_check_loaded_plugins(context.clone())
        .await
        .expect("native deferred health should still run");
    manager
        .shutdown_loaded_plugins(context.clone())
        .await
        .expect("native deferred shutdown should still run");

    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(health_checks.load(Ordering::SeqCst), 1);
    assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
    assert_eq!(unloads.load(Ordering::SeqCst), 1);
    assert!(
        !manager.is_loaded("native-lifecycle"),
        "native deferred plugin should be removed after shutdown"
    );
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
async fn plugin_manager_deferred_native_manifest_runtime_fails_health_check_and_cleans_on_shutdown()
{
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("native_manifest_deferred");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "native-manifest-deferred",
  "name": "Native Manifest Deferred",
  "type": "service"
}"#,
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["native-manifest-deferred".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("deferred native manifest plugin should still load");

    manager
        .start_loaded_plugins(context.clone())
        .await
        .expect("deferred native manifest start should be skipped, not fail");

    let err = manager
        .health_check_loaded_plugins(context.clone())
        .await
        .expect_err("deferred native manifest runtime should fail health check");
    match err {
        PluginLoadError::Lifecycle { id, phase, reason } => {
            assert_eq!(id, "native-manifest-deferred");
            assert_eq!(phase, "health_check");
            assert!(
                reason.contains("manifest runtime is deferred"),
                "unexpected reason: {reason}"
            );
            assert!(
                reason.contains("native plugin entrypoint is not declared"),
                "deferred reason should preserve the native planner detail: {reason}"
            );
        }
        other => panic!("expected Lifecycle error, got {:?}", other),
    }

    assert!(
        manager.is_loaded("native-manifest-deferred"),
        "failed health check should not implicitly unload the plugin"
    );

    manager
        .shutdown_loaded_plugins(context.clone())
        .await
        .expect("deferred native manifest shutdown should skip hooks and clean loaded state");
    assert!(
        !manager.is_loaded("native-manifest-deferred"),
        "shutdown should remove deferred native manifest plugin from loaded state"
    );
}

#[test]
fn plugin_manifest_loader_normalizes_command_scope_alias() {
    let dir = TempDir::create();
    let manifest_path = dir.path.join("plugin.json");
    std::fs::write(
        &manifest_path,
        r#"{
  "id": "manifest-scope-normalize",
  "name": "Manifest Scope Normalize",
  "type": "service",
  "commands": [
    {
      "name": "liteecho",
      "description": "alias scope test",
      "scopes": ["onebot11"]
    }
  ]
}"#,
    )
    .expect("manifest should be written");

    let manifest =
        PluginManifestLoader::load_manifest(&manifest_path).expect("manifest should load");

    assert_eq!(manifest.descriptor.commands.len(), 1);
    assert_eq!(manifest.descriptor.commands[0].name, "/liteecho");
    assert_eq!(
        manifest.descriptor.commands[0].scopes,
        vec!["adapter:onebot11".to_string()]
    );
}

#[test]
fn plugin_manifest_loader_rejects_unknown_command_scope() {
    let dir = TempDir::create();
    let manifest_path = dir.path.join("plugin.json");
    std::fs::write(
        &manifest_path,
        r#"{
  "id": "manifest-scope-invalid",
  "name": "Manifest Scope Invalid",
  "type": "service",
  "commands": [
    {
      "name": "/broken",
      "description": "bad scope",
      "scopes": ["discord"]
    }
  ]
}"#,
    )
    .expect("manifest should be written");

    let err = PluginManifestLoader::load_manifest(&manifest_path)
        .expect_err("unknown scope should be rejected");
    assert!(
        err.to_string().contains("unsupported scope 'discord'"),
        "unexpected error: {err}"
    );
}

#[test]
fn plugin_manifest_loader_rejects_unknown_permission() {
    let dir = TempDir::create();
    let manifest_path = dir.path.join("plugin.json");
    std::fs::write(
        &manifest_path,
        r#"{
  "id": "manifest-permission-invalid",
  "name": "Manifest Permission Invalid",
  "type": "service",
  "permissions": ["filesystem.write"]
}"#,
    )
    .expect("manifest should be written");

    let err = PluginManifestLoader::load_manifest(&manifest_path)
        .expect_err("unknown permission should be rejected");
    assert!(
        err.to_string()
            .contains("unsupported permission 'filesystem.write'"),
        "unexpected error: {err}"
    );
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
    assert_eq!(loaded[0].load_plan.runtime_kind, PluginRuntimeKind::Python);
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Deferred);
    assert_eq!(
        loaded[0].load_plan.contract.abi_name,
        "liteyuki-python-bridge"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_deferred_manifest_runtime_fails_health_check_and_cleans_on_shutdown() {
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_deferred_health");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "python-deferred-health",
  "name": "Python Deferred Health",
  "type": "service",
  "runtime": {
    "kind": "python",
    "entrypoint": "missing_module:bootstrap"
  }
}"#,
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-deferred-health".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("deferred manifest plugin should still load");

    manager
        .start_loaded_plugins(context.clone())
        .await
        .expect("deferred manifest start should be skipped, not fail");

    let err = manager
        .health_check_loaded_plugins(context.clone())
        .await
        .expect_err("deferred manifest runtime should fail health check");
    match err {
        PluginLoadError::Lifecycle { id, phase, reason } => {
            assert_eq!(id, "python-deferred-health");
            assert_eq!(phase, "health_check");
            assert!(
                reason.contains("manifest runtime is deferred"),
                "unexpected reason: {reason}"
            );
            assert!(
                reason.contains("missing_module"),
                "deferred reason should preserve the probe failure detail: {reason}"
            );
        }
        other => panic!("expected Lifecycle error, got {:?}", other),
    }

    assert!(
        manager.is_loaded("python-deferred-health"),
        "failed health check should not implicitly unload the plugin"
    );

    manager
        .shutdown_loaded_plugins(context.clone())
        .await
        .expect("deferred manifest shutdown should skip hooks and clean loaded state");
    assert!(
        !manager.is_loaded("python-deferred-health"),
        "shutdown should remove deferred manifest plugin from loaded state"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_metadata_only_external_manifest_runtime_skips_health_check_failure() {
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let manifest_dir = dir.path.join("manifests");
    let source_root = dir.path.join("nonebot_plugin").join("demo_nonebot");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir should be created");
    std::fs::create_dir_all(&source_root).expect("source root should be created");
    std::fs::write(
        manifest_dir.join("demo.override.json"),
        r#"{
  "version": 1,
  "source": {
    "kind": "nonebot",
    "path": "nonebot_plugin/demo_nonebot"
  },
  "host": {
    "name": "Demo NoneBot"
  }
}"#,
    )
    .expect("override manifest should be written");

    let discovered = manager
        .discover_manifest_plugins_in_dirs([dir.path.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["demo-nonebot".to_string()]);

    let context = plugin_context();
    let loaded = manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("metadata-only manifest should load");
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Deferred);
    assert_eq!(loaded[0].load_plan.runtime_kind, PluginRuntimeKind::External);

    manager
        .health_check_loaded_plugins(context.clone())
        .await
        .expect("metadata-only deferred external runtime should not fail health check");

    manager
        .shutdown_loaded_plugins(context)
        .await
        .expect("metadata-only deferred external runtime should shutdown cleanly");
    assert!(
        !manager.is_loaded("demo-nonebot"),
        "metadata-only deferred plugin should be removed after shutdown"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_nonebot_override_with_runtime_override_still_fails_health_check() {
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let manifest_dir = dir.path.join("manifests");
    let source_root = dir.path.join("nonebot_plugin").join("demo_nonebot");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir should be created");
    std::fs::create_dir_all(&source_root).expect("source root should be created");
    std::fs::write(
        manifest_dir.join("demo.override.json"),
        r#"{
  "version": 1,
  "source": {
    "kind": "nonebot",
    "path": "nonebot_plugin/demo_nonebot"
  },
  "host": {
    "runtime": {
      "abi": "custom-external"
    }
  }
}"#,
    )
    .expect("override manifest should be written");

    let discovered = manager
        .discover_manifest_plugins_in_dirs([dir.path.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["demo-nonebot".to_string()]);

    let context = plugin_context();
    let loaded = manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("overridden metadata-only manifest should still load");
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Deferred);
    assert_eq!(loaded[0].load_plan.runtime_kind, PluginRuntimeKind::External);

    let err = manager
        .health_check_loaded_plugins(context.clone())
        .await
        .expect_err("runtime-overridden synthetic nonebot plugin should fail health check");
    match err {
        PluginLoadError::Lifecycle { id, phase, reason } => {
            assert_eq!(id, "demo-nonebot");
            assert_eq!(phase, "health_check");
            assert!(
                reason.contains("manifest runtime is deferred"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Lifecycle error, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_plain_external_manifest_runtime_still_fails_health_check() {
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("external_manifest_deferred");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "external-deferred-health",
  "name": "External Deferred Health",
  "type": "service",
  "extra": {
    "sourceFamily": "nonebot",
    "adapterFamily": "nonebot_external",
    "compatLevel": "metadata_only",
    "sourcePath": "nonebot_plugin/external_manifest_deferred",
    "overrideManifestPath": "manifests/external_manifest_deferred.override.json"
  },
  "runtime": {
    "kind": "external"
  }
}"#,
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["external-deferred-health".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("external deferred manifest should still load");

    let err = manager
        .health_check_loaded_plugins(context.clone())
        .await
        .expect_err("plain external deferred runtime should still fail health check");
    match err {
        PluginLoadError::Lifecycle { id, phase, reason } => {
            assert_eq!(id, "external-deferred-health");
            assert_eq!(phase, "health_check");
            assert!(
                reason.contains("manifest runtime is deferred"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Lifecycle error, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_rejects_plugin_with_unsupported_sdk_api_version() {
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_api_version");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "python-api-too-new",
  "name": "Python Api Too New",
  "type": "service",
  "runtime": {
    "kind": "python",
    "entrypoint": "echo:main"
  },
  "sdk": {
    "api_version": "0.2"
  }
}"#,
    )
    .expect("manifest should be written");

    let discovered = manager
        .discover_manifest_plugins_in_dirs([dir.path.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-api-too-new".to_string()]);

    let err = manager
        .load_plugins(discovered, plugin_context())
        .await
        .expect_err("unsupported sdk api version should fail planning");
    match err {
        PluginLoadError::Sdk { id, reason } => {
            assert_eq!(id, "python-api-too-new");
            assert!(
                reason.contains("api_version '0.2'"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Sdk error, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_rejects_plugin_with_unmet_min_host_version() {
    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_host_version");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "python-host-too-old",
  "name": "Python Host Too Old",
  "type": "service",
  "runtime": {
    "kind": "python",
    "entrypoint": "echo:main"
  },
  "sdk": {
    "min_host_version": "9.9.9"
  }
}"#,
    )
    .expect("manifest should be written");

    let discovered = manager
        .discover_manifest_plugins_in_dirs([dir.path.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-host-too-old".to_string()]);

    let err = manager
        .load_plugins(discovered, plugin_context())
        .await
        .expect_err("unmet min_host_version should fail planning");
    match err {
        PluginLoadError::Sdk { id, reason } => {
            assert_eq!(id, "python-host-too-old");
            assert!(
                reason.contains("requires host version >= 9.9.9"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Sdk error, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_marks_python_runtime_as_ready_when_module_is_importable() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_ready");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("plugin-config.yaml");
    std::fs::write(&config_path, "plugin:\n  value: 1\n").expect("config file should be written");

    std::fs::write(
        plugin_dir.join("echo_plugin.py"),
        r#"class Meta:
    name = "Rust Bridge Echo"
    type = "service"

__plugin_meta__ = Meta()

def bootstrap(sdk):
    current = sdk.config_get("plugin.value")
    if current is None:
        current = 0
    sdk.config_set("plugin.value", int(current) + 1)

    def _echo(args, runtime_sdk):
        prefix = runtime_sdk.config_get("plugin.value")
        return f"{prefix}:{' '.join(args)}"

    sdk.add_tui_command("/py-echo", _echo, "python echo command")
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "python-echo-ready",
  "name": "Python Echo Ready",
  "type": "service",
  "permissions": [
    "config.read",
    "config_write",
    "command_tui_write"
  ],
  "commands": [
    {{
      "name": "/py-echo",
      "description": "python echo command",
      "scopes": ["tui"]
    }}
  ],
  "runtime": {{
    "kind": "python",
    "entrypoint": "echo_plugin:bootstrap",
    "options": {{
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-echo-ready".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("python descriptor plugin should load in ready mode");

    let loaded = manager.loaded_plugins();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].descriptor.metadata.id, "python-echo-ready");
    assert_eq!(loaded[0].load_plan.runtime_kind, PluginRuntimeKind::Python);
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Ready);
    assert_eq!(
        loaded[0].load_plan.contract.abi_name,
        "liteyuki-python-bridge"
    );

    let command_result = context
        .sdk
        .execute_tui_command("/py-echo", &["hello".to_string(), "world".to_string()])
        .expect("plugin command should execute")
        .expect("plugin command should be registered");
    assert_eq!(command_result, "2:hello world");

    let scoped_commands = context.sdk.list_scope_commands("tui");
    assert!(scoped_commands.iter().any(|command| {
        command.name == "/py-echo"
            && command.plugin_id == "python-echo-ready"
            && command.executable_in_tui
    }));

    let updated_config =
        std::fs::read_to_string(config_path).expect("updated config should stay readable");
    assert!(updated_config.contains("value: 2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python_plan_load_does_not_import_module_before_runtime_activation() {
    if !python_command_available() {
        return;
    }

    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plan_probe");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let marker_path = dir.path.join("plan-side-effect.txt");
    let marker_literal = marker_path.to_string_lossy().replace('\\', "/");

    std::fs::write(
        plugin_dir.join("side_effect_plugin.py"),
        format!(
            r#"from pathlib import Path

Path("{}").write_text("imported", encoding="utf-8")

def bootstrap(sdk):
    return None
"#,
            marker_literal
        ),
    )
    .expect("python module should be written");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "python-plan-side-effect",
  "name": "Python Plan Side Effect",
  "type": "service",
  "runtime": {
    "kind": "python",
    "entrypoint": "side_effect_plugin:bootstrap"
  }
}"#,
    )
    .expect("manifest should be written");

    let manifest = PluginManifestLoader::load_manifest(plugin_dir.join("plugin.json").as_path())
        .expect("manifest should load");
    let context = plugin_context();

    let plan = context
        .sdk
        .plan_load(&manifest.descriptor, &context.host)
        .await
        .expect("plan should succeed");
    assert_eq!(plan.state, PluginLoadState::Ready);
    assert!(
        !marker_path.exists(),
        "plan_load should not import python modules or trigger top-level side effects"
    );

    let activated = context
        .sdk
        .load_manifest_plugin(&manifest.descriptor, &context.host)
        .expect("runtime activation should succeed");
    assert!(activated, "python runtime should activate during load");
    assert!(
        marker_path.exists(),
        "runtime activation should still import the module during actual load"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python_unload_cleans_sys_modules_for_package_and_submodule() {
    if !python_command_available() {
        return;
    }

    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_unload_cleanup");
    let package_dir = plugin_dir.join("reload_pkg");
    std::fs::create_dir_all(&package_dir).expect("package dir should be created");

    std::fs::write(
        package_dir.join("__init__.py"),
        r#"from . import helper

def bootstrap(sdk):
    return helper.VALUE
"#,
    )
    .expect("package init should be written");
    std::fs::write(package_dir.join("helper.py"), "VALUE = 1\n")
        .expect("package helper should be written");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "python-unload-module-cleanup",
  "name": "Python Unload Module Cleanup",
  "type": "service",
  "runtime": {
    "kind": "python",
    "entrypoint": "reload_pkg:bootstrap"
  }
}"#,
    )
    .expect("manifest should be written");

    let manifest = PluginManifestLoader::load_manifest(plugin_dir.join("plugin.json").as_path())
        .expect("manifest should load");
    let context = plugin_context();

    let activated = context
        .sdk
        .load_manifest_plugin(&manifest.descriptor, &context.host)
        .expect("runtime activation should succeed");
    assert!(activated, "python runtime should activate during load");

    Python::with_gil(|py| {
        let sys = py.import("sys").expect("sys should import");
        let modules = sys
            .getattr("modules")
            .expect("sys.modules should exist")
            .downcast_into::<pyo3::types::PyDict>()
            .expect("sys.modules should be a dict");
        let package_loaded = modules
            .call_method1("__contains__", ("reload_pkg",))
            .expect("dict contains should work")
            .is_truthy()
            .expect("truthy check should work");
        let helper_loaded = modules
            .call_method1("__contains__", ("reload_pkg.helper",))
            .expect("dict contains should work")
            .is_truthy()
            .expect("truthy check should work");
        assert!(package_loaded, "package module should be loaded");
        assert!(helper_loaded, "submodule should be loaded");
    });

    context
        .sdk
        .unload_manifest_plugin(&manifest.descriptor)
        .expect("unload should succeed");

    Python::with_gil(|py| {
        let sys = py.import("sys").expect("sys should import");
        let modules = sys
            .getattr("modules")
            .expect("sys.modules should exist")
            .downcast_into::<pyo3::types::PyDict>()
            .expect("sys.modules should be a dict");
        let package_loaded = modules
            .call_method1("__contains__", ("reload_pkg",))
            .expect("dict contains should work")
            .is_truthy()
            .expect("truthy check should work");
        let helper_loaded = modules
            .call_method1("__contains__", ("reload_pkg.helper",))
            .expect("dict contains should work")
            .is_truthy()
            .expect("truthy check should work");
        assert!(
            !package_loaded,
            "package module should be removed from sys.modules on unload"
        );
        assert!(
            !helper_loaded,
            "package submodule should be removed from sys.modules on unload"
        );
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_rejects_python_plugin_bootstrap_without_required_permission() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_permission_denied");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("permission-denied-config.yaml");
    std::fs::write(&config_path, "plugin:\n  value: 1\n").expect("config file should be written");

    std::fs::write(
        plugin_dir.join("permission_denied.py"),
        r#"def bootstrap(sdk):
    sdk.config_set("plugin.value", 2)
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "python-permission-denied",
  "name": "Python Permission Denied",
  "type": "service",
  "permissions": [],
  "runtime": {{
    "kind": "python",
    "entrypoint": "permission_denied:bootstrap",
    "options": {{
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-permission-denied".to_string()]);

    let err = manager
        .load_plugins(discovered, plugin_context())
        .await
        .expect_err("bootstrap should fail without config.write permission");
    match err {
        PluginLoadError::Hook { id, reason } => {
            assert_eq!(id, "python-permission-denied");
            assert!(
                reason.contains("missing permission 'config.write'"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Hook error, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_runs_python_start_health_shutdown_and_unload_hooks() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_lifecycle");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("plugin-lifecycle.yaml");
    std::fs::write(&config_path, "plugin:\n  load_count: 0\n")
        .expect("config file should be written");

    std::fs::write(
        plugin_dir.join("lifecycle_plugin.py"),
        r#"def bootstrap(sdk):
    current = sdk.config_get("plugin.load_count")
    if current is None:
        current = 0
    sdk.config_set("plugin.load_count", int(current) + 1)

def on_start(sdk):
    sdk.config_set("plugin.started", True)

def on_health_check(sdk):
    sdk.config_set("plugin.healthy", True)

def on_shutdown(sdk):
    sdk.config_set("plugin.stopped", True)

def on_unload(sdk):
    sdk.config_set("plugin.unloaded", True)
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "python-lifecycle",
  "name": "Python Lifecycle",
  "type": "service",
  "permissions": ["config.read", "config.write"],
  "runtime": {{
    "kind": "python",
    "entrypoint": "lifecycle_plugin:bootstrap",
    "options": {{
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-lifecycle".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("python lifecycle plugin should load");
    manager
        .start_loaded_plugins(context.clone())
        .await
        .expect("start hooks should run");
    manager
        .health_check_loaded_plugins(context.clone())
        .await
        .expect("health hooks should run");
    manager
        .shutdown_loaded_plugins(context.clone())
        .await
        .expect("shutdown hooks should run");

    assert!(
        manager.loaded_plugins().is_empty(),
        "loaded registry should be cleared after shutdown"
    );

    let updated_config =
        std::fs::read_to_string(config_path).expect("updated config should stay readable");
    assert!(updated_config.contains("load_count: 1"));
    assert!(updated_config.contains("started: true"));
    assert!(updated_config.contains("healthy: true"));
    assert!(updated_config.contains("stopped: true"));
    assert!(updated_config.contains("unloaded: true"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_health_check_failure_does_not_unload_plugin() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_health_failure");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");

    std::fs::write(
        plugin_dir.join("health_failure_plugin.py"),
        r#"def bootstrap(sdk):
    def _still_loaded(args, runtime_sdk):
        return "still-loaded"

    sdk.add_tui_command("/py-still-loaded", _still_loaded, "still loaded command")

def on_health_check(sdk):
    raise RuntimeError("intentional health failure")
"#,
    )
    .expect("python module should be written");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "python-health-failure",
  "name": "Python Health Failure",
  "type": "service",
  "permissions": ["command.tui.manage"],
  "commands": [
    {
      "name": "/py-still-loaded",
      "description": "still loaded command",
      "scopes": ["tui"]
    }
  ],
  "runtime": {
    "kind": "python",
    "entrypoint": "health_failure_plugin:bootstrap"
  }
}"#,
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-health-failure".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("python health failure plugin should load");

    let err = manager
        .health_check_loaded_plugins(context.clone())
        .await
        .expect_err("health check should fail");
    match err {
        PluginLoadError::Lifecycle { id, phase, reason } => {
            assert_eq!(id, "python-health-failure");
            assert_eq!(phase, "health_check");
            assert!(
                reason.contains("intentional health failure"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Lifecycle error, got {:?}", other),
    }

    assert!(
        manager.is_loaded("python-health-failure"),
        "health failure should not implicitly unload the plugin"
    );
    let command_result = context
        .sdk
        .execute_tui_command("/py-still-loaded", &[])
        .expect("command lookup should succeed")
        .expect("command should remain registered after failed health check");
    assert_eq!(command_result, "still-loaded");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_unload_failure_still_cleans_python_runtime_state() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_unload_failure");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");

    std::fs::write(
        plugin_dir.join("unload_failure_plugin.py"),
        r#"def bootstrap(sdk):
    def _unload_probe(args, runtime_sdk):
        return "before-unload"

    sdk.add_tui_command("/py-unload-probe", _unload_probe, "unload probe command")

def on_unload(sdk):
    raise RuntimeError("intentional unload failure")
"#,
    )
    .expect("python module should be written");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "python-unload-failure",
  "name": "Python Unload Failure",
  "type": "service",
  "permissions": ["command.tui.manage"],
  "commands": [
    {
      "name": "/py-unload-probe",
      "description": "unload probe command",
      "scopes": ["tui"]
    }
  ],
  "runtime": {
    "kind": "python",
    "entrypoint": "unload_failure_plugin:bootstrap"
  }
}"#,
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-unload-failure".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("python unload failure plugin should load");

    let before_unload = context
        .sdk
        .execute_tui_command("/py-unload-probe", &[])
        .expect("command lookup should succeed before unload")
        .expect("command should be registered before unload");
    assert_eq!(before_unload, "before-unload");

    let err = manager
        .shutdown_loaded_plugins(context.clone())
        .await
        .expect_err("unload failure should surface");
    match err {
        PluginLoadError::Lifecycle { id, phase, reason } => {
            assert_eq!(id, "python-unload-failure");
            assert_eq!(phase, "unload");
            assert!(
                reason.contains("intentional unload failure"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Lifecycle error, got {:?}", other),
    }

    assert!(
        !manager.is_loaded("python-unload-failure"),
        "manager should clear loaded state even when unload fails"
    );
    assert!(
        context.sdk.get_tui_command("/py-unload-probe").is_none(),
        "runtime command registry should be cleaned during unload"
    );
    let after_unload = context
        .sdk
        .execute_tui_command("/py-unload-probe", &[])
        .expect("command lookup after unload should succeed");
    assert!(
        after_unload.is_none(),
        "runtime command handler should be removed during unload cleanup"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_loads_legacy_liteecho_python_plugin() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("legacy_liteecho");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");

    std::fs::write(
        plugin_dir.join("liteecho_slash.py"),
        r#"# -*- coding: utf-8 -*-
from liteyuki.session.on import on_startswith
from liteyuki.session.event import MessageEvent
from liteyuki.session.rule import is_su_rule

@on_startswith(["liteecho"], rule=is_su_rule).handle()
async def liteecho(event: MessageEvent):
    event.reply(event.raw_message.strip()[8:].strip())
"#,
    )
    .expect("python module should be written");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "legacy-liteecho",
  "name": "Legacy LiteEcho",
  "type": "service",
  "permissions": ["adapter.reply"],
  "commands": [
    {
      "name": "/liteecho",
      "description": "legacy echo command",
      "scopes": ["adapter:onebot11"]
    }
  ],
  "runtime": {
    "kind": "python",
    "entrypoint": "liteecho_slash",
    "options": {
      "event_handler": "liteyuki_handle_event"
    }
  }
}"#,
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["legacy-liteecho".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("legacy liteecho plugin should load");

    let loaded = manager.loaded_plugins();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].descriptor.metadata.id, "legacy-liteecho");
    assert_eq!(loaded[0].load_plan.runtime_kind, PluginRuntimeKind::Python);
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Ready);
    assert!(
        context
            .sdk
            .list_scope_commands("adapter:onebot11")
            .iter()
            .any(|command| {
                command.name == "/liteecho"
                    && command.plugin_id == "legacy-liteecho"
                    && !command.executable_in_tui
            })
    );

    context.sdk.dispatch_event(
        &BotEvent::new(
            42,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "group",
                "group_id": "10001",
                "user_id": "20001",
                "raw_message": "/liteecho hello from legacy"
            }),
        ),
        &context.logger,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_legacy_on_startswith_matches_slash_prefixed_command() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("legacy_liteecho_slash");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("legacy-plugin-config.yaml");
    std::fs::write(&config_path, "plugin:\n  hit: false\n").expect("config file should be written");

    std::fs::write(
        plugin_dir.join("liteecho.py"),
        r#"# -*- coding: utf-8 -*-
from liteyuki.session.on import on_startswith
from liteyuki.session.event import MessageEvent
from liteyuki.session.rule import is_su_rule
import liteyuki

@on_startswith(["liteecho"], rule=is_su_rule).handle()
async def liteecho(event: MessageEvent):
    event._sdk.config_set("plugin.hit", True)
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "legacy-liteecho-slash",
  "name": "Legacy LiteEcho Slash",
  "type": "service",
  "permissions": ["config.write"],
  "runtime": {{
    "kind": "python",
    "entrypoint": "liteecho",
    "options": {{
      "event_handler": "liteyuki_handle_event",
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["legacy-liteecho-slash".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("legacy liteecho slash plugin should load");

    context.sdk.dispatch_event(
        &BotEvent::new(
            100,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "private",
                "user_id": "20001",
                "raw_message": "/liteecho slash-test"
            }),
        ),
        &context.logger,
    );

    let updated_config =
        std::fs::read_to_string(config_path).expect("updated config should stay readable");
    assert!(
        updated_config.contains("hit: true"),
        "slash-prefixed command should trigger legacy on_startswith handler"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_legacy_on_message_dispatches_all_messages() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("legacy_on_message");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("legacy-on-message-config.yaml");
    std::fs::write(&config_path, "plugin:\n  hit: false\n").expect("config file should be written");

    std::fs::write(
        plugin_dir.join("hello_liteyuki.py"),
        r#"# -*- coding: utf-8 -*-
from liteyuki.session.on import on_message
from liteyuki.session.event import MessageEvent

@on_message().handle()
async def handle_message(event: MessageEvent):
    event._sdk.config_set("plugin.hit", True)
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "legacy-on-message",
  "name": "Legacy On Message",
  "type": "application",
  "permissions": ["config.write"],
  "runtime": {{
    "kind": "python",
    "entrypoint": "hello_liteyuki",
    "options": {{
      "event_handler": "liteyuki_handle_event",
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["legacy-on-message".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("legacy on_message plugin should load");

    context.sdk.dispatch_event(
        &BotEvent::new(
            100,
            "runtime.lifecycle",
            json!({
                "phase": "loaded"
            }),
        ),
        &context.logger,
    );
    let unchanged_config =
        std::fs::read_to_string(&config_path).expect("config should stay readable");
    assert!(
        unchanged_config.contains("hit: false"),
        "on_message handler should skip non-message runtime events"
    );

    context.sdk.dispatch_event(
        &BotEvent::new(
            101,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "private",
                "user_id": "20001",
                "raw_message": "hello liteyuki"
            }),
        ),
        &context.logger,
    );

    let updated_config =
        std::fs::read_to_string(config_path).expect("updated config should stay readable");
    assert!(
        updated_config.contains("hit: true"),
        "on_message handler should receive matching adapter messages"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_loads_builtin_hello_liteyuki_python_plugin() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let plugin_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("builtin_plugin");
    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("builtin manifest discovery should succeed");
    assert!(
        discovered
            .iter()
            .any(|plugin_id| plugin_id == "builtin-hello-liteyuki"),
        "builtin root discovery should include hello_liteyuki"
    );

    manager
        .load_plugins(["builtin-hello-liteyuki".to_string()], context.clone())
        .await
        .expect("builtin hello_liteyuki plugin should load");

    let loaded = manager.loaded_plugins();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].descriptor.metadata.id, "builtin-hello-liteyuki");
    assert_eq!(loaded[0].load_plan.runtime_kind, PluginRuntimeKind::Python);
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Ready);

    context.sdk.dispatch_event(
        &BotEvent::new(
            102,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "private",
                "user_id": "20001",
                "raw_message": "你好轻雪"
            }),
        ),
        &context.logger,
    );

    let mut reply_attempted = false;
    for _ in 0..10 {
        reply_attempted = recent_buffered_logs(30).iter().any(|entry| {
            entry.module == "plugin.python"
                && entry.message.contains("builtin-hello-liteyuki")
                && entry.message.contains("onebot reply send failed")
        });
        if reply_attempted {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert!(
        reply_attempted,
        "hello_liteyuki should reach event.reply and attempt a onebot reply"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_loads_astrbot_style_python_plugin_with_combined_filters() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("astrbot_style_echo");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("astrbot-style-config.yaml");
    std::fs::write(
        &config_path,
        "plugin:\n  loaded: false\n  group_hits: 0\n  private_hits: 0\n",
    )
    .expect("config file should be written");

    std::fs::write(
        plugin_dir.join("astr_echo_plugin.py"),
        r#"from astrbot.api import logger, star
from astrbot.api.event import AstrMessageEvent, filter


class AstrCompatEcho(star.Star):
    def __init__(self, context: star.Context, config=None):
        super().__init__(context, config)
        self.context = context
        logger.info("AstrCompatEcho initialized")

    @filter.on_astrbot_loaded()
    async def on_ready(self):
        self.context.config_set("plugin.loaded", True)

    @filter.event_message_type(filter.EventMessageType.GROUP_MESSAGE)
    @filter.command("echo", alias={"astr-echo"})
    async def handle_group_echo(self, event: AstrMessageEvent):
        current = self.context.config_get("plugin.group_hits", 0)
        if current is None:
            current = 0
        self.context.config_set("plugin.group_hits", int(current) + 1)
        self.context.config_set("plugin.last_group_message", event.get_message_str())
        event.stop_event()

    @filter.event_message_type(filter.EventMessageType.PRIVATE_MESSAGE)
    @filter.command("echo")
    async def handle_private_echo(self, event: AstrMessageEvent):
        current = self.context.config_get("plugin.private_hits", 0)
        if current is None:
            current = 0
        self.context.config_set("plugin.private_hits", int(current) + 1)
"#,
    )
    .expect("python module should be written");

    let config_path_json = config_path.to_string_lossy().replace('\\', "/");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "astrbot-style-echo",
  "name": "AstrBot Style Echo",
  "type": "service",
  "permissions": ["config.read", "config.write"],
  "commands": [
    {{
      "name": "/echo",
      "description": "astrbot echo command",
      "scopes": ["adapter:onebot11"]
    }}
  ],
  "runtime": {{
    "kind": "python",
    "entrypoint": "astr_echo_plugin",
    "options": {{
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["astrbot-style-echo".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("astrbot style plugin should load");

    let loaded = manager.loaded_plugins();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].descriptor.metadata.id, "astrbot-style-echo");
    assert_eq!(loaded[0].load_plan.runtime_kind, PluginRuntimeKind::Python);
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Ready);

    manager
        .start_loaded_plugins(context.clone())
        .await
        .expect("astrbot style start hooks should run");

    context.sdk.dispatch_event(
        &BotEvent::new(
            201,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "group",
                "group_id": "10001",
                "user_id": "20001",
                "raw_message": "/echo grouped"
            }),
        ),
        &context.logger,
    );
    context.sdk.dispatch_event(
        &BotEvent::new(
            202,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "private",
                "user_id": "20001",
                "raw_message": "/echo private"
            }),
        ),
        &context.logger,
    );

    let updated_config =
        std::fs::read_to_string(config_path).expect("updated config should stay readable");
    assert!(
        updated_config.contains("loaded: true"),
        "astrbot on_astrbot_loaded hook should update config"
    );
    assert!(
        updated_config.contains("group_hits: 1"),
        "group command handler should run exactly once"
    );
    assert!(
        updated_config.contains("private_hits: 1"),
        "private command handler should run exactly once"
    );
    assert!(
        updated_config.contains("last_group_message: /echo grouped"),
        "group handler should see the original message text"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_astrbot_message_event_result_replies_and_injects_command_args() {
    if !python_command_available() {
        return;
    }

    let (endpoint_url, capture_task) = start_http_capture_server().await;

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("astrbot_result_echo");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");

    std::fs::write(
        plugin_dir.join("astr_result_plugin.py"),
        r#"from astrbot.api import star
from astrbot.api.event import AstrMessageEvent, MessageEventResult, filter


class AstrCompatResult(star.Star):
    @filter.command("sum")
    async def handle_sum(self, event: AstrMessageEvent, left, right):
        return MessageEventResult().message(f"{left}+{right}")
"#,
    )
    .expect("python module should be written");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "astrbot-result-echo",
  "name": "AstrBot Result Echo",
  "type": "service",
  "permissions": ["adapter.reply"],
  "runtime": {
    "kind": "python",
    "entrypoint": "astr_result_plugin"
  }
}"#,
    )
    .expect("manifest should be written");

    let context = plugin_context();
    context
        .host
        .adapter_manager()
        .register(AdapterConfig {
            id: "reply-adapter".to_string(),
            enabled: true,
            transport: AdapterTransport::Http,
            endpoint: AdapterEndpoint {
                url: endpoint_url,
                headers: HashMap::new(),
                token: None,
                timeout_ms: 5_000,
            },
            ..AdapterConfig::default()
        })
        .expect("reply adapter should register");

    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["astrbot-result-echo".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("astrbot result plugin should load");

    context.sdk.dispatch_event(
        &BotEvent::new(
            301,
            "adapter.inbound",
            json!({
                "_adapter_id": "reply-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "private",
                "user_id": "20001",
                "raw_message": "/sum alpha beta"
            }),
        ),
        &context.logger,
    );

    let request = timeout(Duration::from_secs(3), capture_task)
        .await
        .expect("reply capture should complete")
        .expect("reply capture task should join");
    assert!(
        request.contains("\"message\":\"alpha+beta\""),
        "MessageEventResult reply should include parsed command args: {request}"
    );
    assert!(
        request.contains("\"action\":\"send_msg\""),
        "reply path should produce a onebot send_msg payload: {request}"
    );
}

#[test]
fn plugin_manager_astrbot_context_exposes_tool_and_schedule_metadata() {
    if !python_command_available() {
        return;
    }

    let dir = TempDir::create();
    let plugin_dir = dir.path.join("astrbot_context_tools");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("astrbot-context-tools.yaml");
    std::fs::write(&config_path, "plugin: {}\n").expect("config file should be written");

    std::fs::write(
        plugin_dir.join("astr_context_tools.py"),
        r#"import liteyuki
from astrbot.api import FunctionTool, star
from astrbot.api.event import filter


class ManualTool(FunctionTool):
    def __init__(self):
        super().__init__(
            name="manual_tool_for_context_test",
            description="manual tool",
            parameters={"type": "object", "properties": {}},
            handler=self.run,
        )

    async def run(self):
        return "ok"


class AstrCompatContextTools(star.Star):
    async def initialize(self):
        manager = self.context.get_llm_tool_manager()
        runtime = liteyuki._get_astrbot_plugin_runtime(__name__)
        self.context.register_web_api("/compat-tools", self.handle_api, ["POST"], "compat tools api")
        self.context.add_llm_tools(
            FunctionTool(
                name="context_tool_for_context_test",
                description="context tool",
                parameters={
                    "type": "object",
                    "properties": {"value": {"type": "string"}},
                },
                handler=self.context_tool,
            ),
            ManualTool(),
        )
        await self.context.cron_manager.add_active_job(
            name="compat-cron-job",
            description="compat cron",
            cron_expression="0 0 * * *",
            payload={"note": "compat"},
            enabled=True,
        )
        self.context.register_task("legacy-task-token", "legacy task")

        self.context.deactivate_llm_tool("decorated_tool_for_context_test")
        inactive = manager.get_func("decorated_tool_for_context_test").active
        self.context.activate_llm_tool("decorated_tool_for_context_test")
        active = manager.get_func("decorated_tool_for_context_test").active
        jobs = await self.context.cron_manager.list_jobs()
        decorated = manager.get_func("decorated_tool_for_context_test")

        self.context.config_set(
            "plugin.tool_names",
            [tool.name for tool in runtime["llm_tools"]],
        )
        self.context.config_set(
            "plugin.decorated_query_type",
            decorated.parameters["properties"]["query"]["type"],
        )
        self.context.config_set(
            "plugin.decorated_required",
            decorated.parameters.get("required", []),
        )
        self.context.config_set(
            "plugin.decorated_handler_module_path",
            decorated.handler_module_path,
        )
        self.context.config_set("plugin.decorated_inactive", inactive)
        self.context.config_set("plugin.decorated_active", active)
        self.context.config_set(
            "plugin.web_api_route",
            self.context.registered_web_apis[0][0],
        )
        self.context.config_set(
            "plugin.web_api_methods",
            self.context.registered_web_apis[0][2],
        )
        self.context.config_set("plugin.cron_job_type", jobs[0].job_type)
        self.context.config_set("plugin.cron_expression", jobs[0].cron_expression)
        self.context.config_set(
            "plugin.task_desc",
            self.context._register_tasks[0]["desc"],
        )

    async def handle_api(self):
        return {"ok": True}

    async def context_tool(self, value: str):
        return value

    @filter.llm_tool("decorated_tool_for_context_test")
    async def decorated_tool(self, query: str, count: int = 1):
        return f"{query}:{count}"
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "astrbot-context-tools",
  "name": "AstrBot Context Tools",
  "type": "service",
  "permissions": ["config.write"],
  "runtime": {{
    "kind": "python",
    "entrypoint": "astr_context_tools",
    "options": {{
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let manifest = PluginManifestLoader::load_manifest(plugin_dir.join("plugin.json").as_path())
        .expect("manifest should load");
    let context = plugin_context();

    let activated = context
        .sdk
        .load_manifest_plugin(&manifest.descriptor, &context.host)
        .expect("python runtime should load");
    assert!(
        activated,
        "python runtime should activate during direct load"
    );
    context
        .sdk
        .start_manifest_plugin(&manifest.descriptor)
        .expect("astrbot initialize hook should run");

    let snapshot = context
        .sdk
        .get_plugin_capabilities("astrbot-context-tools")
        .expect("capability snapshot query should succeed")
        .expect("python capability snapshot should exist");
    assert_eq!(snapshot.plugin_id, "astrbot-context-tools");
    assert_eq!(snapshot.runtime_kind, PluginRuntimeKind::Python);
    assert_eq!(snapshot.tools.len(), 3);
    assert_eq!(snapshot.web_apis.len(), 1);
    assert_eq!(snapshot.cron_jobs.len(), 1);
    assert_eq!(snapshot.tasks.len(), 1);
    assert!(
        snapshot
            .tools
            .iter()
            .any(|tool| tool.name == "decorated_tool_for_context_test"
                && tool.source == liteyukibot_core::PluginCapabilitySource::AstrbotDecorator)
    );
    assert!(
        snapshot
            .tools
            .iter()
            .any(|tool| tool.name == "context_tool_for_context_test"
                && tool.source == liteyukibot_core::PluginCapabilitySource::AstrbotContext)
    );
    assert_eq!(snapshot.web_apis[0].route, "/compat-tools");
    assert_eq!(
        snapshot.web_apis[0].source,
        liteyukibot_core::PluginCapabilitySource::AstrbotContext
    );
    assert_eq!(snapshot.cron_jobs[0].job_type, "active_agent");
    assert_eq!(snapshot.tasks[0].task_id, "legacy-task-token");

    let updated = context
        .sdk
        .read_explicit_config_document(&manifest.descriptor)
        .expect("config should stay readable");
    let plugin = &updated["plugin"];
    assert_eq!(
        plugin["tool_names"],
        json!([
            "decorated_tool_for_context_test",
            "context_tool_for_context_test",
            "manual_tool_for_context_test"
        ])
    );
    assert_eq!(plugin["decorated_query_type"], json!("string"));
    assert_eq!(plugin["decorated_required"], json!(["query"]));
    assert_eq!(
        plugin["decorated_handler_module_path"],
        json!("astr_context_tools")
    );
    assert_eq!(plugin["decorated_inactive"], json!(false));
    assert_eq!(plugin["decorated_active"], json!(true));
    assert_eq!(plugin["web_api_route"], json!("/compat-tools"));
    assert_eq!(plugin["web_api_methods"], json!(["POST"]));
    assert_eq!(plugin["cron_job_type"], json!("active_agent"));
    assert_eq!(plugin["cron_expression"], json!("0 0 * * *"));
    assert_eq!(plugin["task_desc"], json!("legacy task"));
}

#[test]
fn plugin_manager_astrbot_command_group_aliases_reach_nested_subcommands() {
    if !python_command_available() {
        return;
    }

    let dir = TempDir::create();
    let plugin_dir = dir.path.join("astrbot_group_aliases");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("astrbot-group-aliases.yaml");
    std::fs::write(&config_path, "plugin:\n  last_message: \"\"\n")
        .expect("config file should be written");

    std::fs::write(
        plugin_dir.join("astr_group_aliases.py"),
        r#"from astrbot.api import star
from astrbot.api.event import AstrMessageEvent, filter

root = filter.command_group("admin", alias={"a"})(object)
tools = root.group("tools", alias={"t"})(object)


class AstrCompatGroupAliases(star.Star):
    @tools.command("ping", alias={"p"})
    async def handle_ping(self, event: AstrMessageEvent):
        self.context.config_set("plugin.last_message", event.get_message_str())
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "astrbot-group-aliases",
  "name": "AstrBot Group Aliases",
  "type": "service",
  "permissions": ["config.write"],
  "runtime": {{
    "kind": "python",
    "entrypoint": "astr_group_aliases",
    "options": {{
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let manifest = PluginManifestLoader::load_manifest(plugin_dir.join("plugin.json").as_path())
        .expect("manifest should load");
    let context = plugin_context();

    let activated = context
        .sdk
        .load_manifest_plugin(&manifest.descriptor, &context.host)
        .expect("python runtime should load");
    assert!(
        activated,
        "python runtime should activate during direct load"
    );

    context.sdk.dispatch_event(
        &BotEvent::new(
            401,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "private",
                "user_id": "20001",
                "raw_message": "/a t p nested alias"
            }),
        ),
        &context.logger,
    );

    let updated = context
        .sdk
        .read_explicit_config_document(&manifest.descriptor)
        .expect("config should stay readable");
    assert_eq!(
        updated["plugin"]["last_message"],
        json!("/a t p nested alias")
    );
}

#[test]
fn plugin_sdk_reuses_single_async_loop_and_cleans_background_tasks() {
    if !python_command_available() {
        return;
    }

    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_async_loop_consistency");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("py-async-loop-consistency.yaml");
    std::fs::write(
        &config_path,
        "plugin:\n  same_loop: false\n  pending_tasks: 1\n",
    )
    .expect("config file should be written");

    std::fs::write(
        plugin_dir.join("async_loop_plugin.py"),
        r#"import asyncio

_start_loop_id = None


async def on_start(sdk):
    global _start_loop_id
    loop = asyncio.get_running_loop()
    _start_loop_id = id(loop)

    async def sleeper():
        try:
            await asyncio.sleep(60)
        except asyncio.CancelledError:
            return None

    asyncio.create_task(sleeper())


async def on_health_check(sdk):
    loop = asyncio.get_running_loop()
    sdk.config_set("plugin.same_loop", id(loop) == _start_loop_id)
    pending = [
        task
        for task in asyncio.all_tasks(loop)
        if task is not asyncio.current_task(loop)
    ]
    sdk.config_set("plugin.pending_tasks", len(pending))
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "python-async-loop-consistency",
  "name": "Python Async Loop Consistency",
  "type": "service",
  "permissions": ["config.write"],
  "runtime": {{
    "kind": "python",
    "entrypoint": "async_loop_plugin",
    "options": {{
      "config_path": "{}",
      "start_handler": "on_start",
      "health_handler": "on_health_check"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let manifest = PluginManifestLoader::load_manifest(plugin_dir.join("plugin.json").as_path())
        .expect("manifest should load");
    let context = plugin_context();

    let activated = context
        .sdk
        .load_manifest_plugin(&manifest.descriptor, &context.host)
        .expect("python async consistency plugin should load");
    assert!(
        activated,
        "python runtime should activate during direct load"
    );
    context
        .sdk
        .start_manifest_plugin(&manifest.descriptor)
        .expect("start hooks should run");
    context
        .sdk
        .health_check_manifest_plugin(&manifest.descriptor)
        .expect("health hooks should run");

    let updated_config =
        std::fs::read_to_string(config_path).expect("updated config should stay readable");
    assert!(
        updated_config.contains("same_loop: true"),
        "async lifecycle hooks should reuse the same dedicated loop"
    );
    assert!(
        updated_config.contains("pending_tasks: 0"),
        "background tasks created in a callback should be cancelled before the next callback"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_can_disable_and_reenable_scoped_adapter_command() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("legacy_liteecho_toggle");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("legacy-toggle-config.yaml");
    std::fs::write(&config_path, "plugin:\n  hit: false\n").expect("config file should be written");

    std::fs::write(
        plugin_dir.join("liteecho.py"),
        r#"# -*- coding: utf-8 -*-
from liteyuki.session.on import on_startswith
from liteyuki.session.event import MessageEvent
from liteyuki.session.rule import is_su_rule

@on_startswith(["liteecho"], rule=is_su_rule).handle()
async def liteecho(event: MessageEvent):
    event._sdk.config_set("plugin.hit", True)
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "legacy-liteecho-toggle",
  "name": "Legacy LiteEcho Toggle",
  "type": "service",
  "permissions": ["config.write"],
  "commands": [
    {{
      "name": "/liteecho",
      "description": "legacy echo command",
      "scopes": ["adapter:onebot11"]
    }}
  ],
  "runtime": {{
    "kind": "python",
    "entrypoint": "liteecho",
    "options": {{
      "event_handler": "liteyuki_handle_event",
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["legacy-liteecho-toggle".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("legacy liteecho toggle plugin should load");

    context
        .sdk
        .set_scope_command_enabled("adapter:onebot11", "/liteecho", false)
        .expect("command disable should succeed");
    context.sdk.dispatch_event(
        &BotEvent::new(
            101,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "private",
                "user_id": "20001",
                "raw_message": "/liteecho disabled"
            }),
        ),
        &context.logger,
    );

    let disabled_config =
        std::fs::read_to_string(&config_path).expect("disabled config should stay readable");
    assert!(
        disabled_config.contains("hit: false"),
        "disabled scoped adapter command should not dispatch into python handler"
    );

    context
        .sdk
        .set_scope_command_enabled("adapter:onebot11", "/liteecho", true)
        .expect("command enable should succeed");
    context.sdk.dispatch_event(
        &BotEvent::new(
            102,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "_adapter_protocol": "onebot.v11",
                "post_type": "message",
                "message_type": "private",
                "user_id": "20001",
                "raw_message": "/liteecho enabled"
            }),
        ),
        &context.logger,
    );

    let enabled_config =
        std::fs::read_to_string(config_path).expect("enabled config should stay readable");
    assert!(
        enabled_config.contains("hit: true"),
        "reenabled scoped adapter command should dispatch again"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_python_sdk_can_disable_builtin_command_and_delete_config_value() {
    if !python_command_available() {
        return;
    }

    let manager = PluginManager::new();
    let dir = TempDir::create();
    let plugin_dir = dir.path.join("py_plugin_controls");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    let config_path = dir.path.join("plugin-controls.yaml");
    std::fs::write(&config_path, "plugin:\n  remove_me: stale\n  keep_me: ok\n")
        .expect("config file should be written");

    std::fs::write(
        plugin_dir.join("controls_plugin.py"),
        r#"class Meta:
    name = "Rust Bridge Controls"
    type = "service"

__plugin_meta__ = Meta()

def bootstrap(sdk):
    sdk.config_delete("plugin.remove_me")
    sdk.disable_tui_command("/help")
"#,
    )
    .expect("python module should be written");
    let config_path_json = config_path.to_string_lossy().replace('\\', "/");

    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "python-controls-ready",
  "name": "Python Controls Ready",
  "type": "service",
  "permissions": ["config.write", "command.tui.manage"],
  "runtime": {{
    "kind": "python",
    "entrypoint": "controls_plugin:bootstrap",
    "options": {{
      "config_path": "{}"
    }}
  }}
}}"#,
            config_path_json
        ),
    )
    .expect("manifest should be written");

    let context = plugin_context();
    let discovered = manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");
    assert_eq!(discovered, vec!["python-controls-ready".to_string()]);

    manager
        .load_plugins(discovered, context.clone())
        .await
        .expect("python descriptor plugin should load in ready mode");

    assert!(context.sdk.is_builtin_tui_command_disabled("/help"));

    let updated_config =
        std::fs::read_to_string(config_path).expect("updated config should stay readable");
    assert!(!updated_config.contains("remove_me"));
    assert!(updated_config.contains("keep_me: ok"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_prevents_reentrant_load_for_same_id() {
    let manager = PluginManager::new();
    let loaded = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());

    manager
        .register_plugin(BlockingPlugin {
            id: "reentrant".to_string(),
            name: "Reentrant Plugin".to_string(),
            loaded: Arc::clone(&loaded),
            release: Arc::clone(&release),
        })
        .expect("register should succeed");

    let manager_clone = manager.clone();
    let first_handle = tokio::spawn(async move {
        manager_clone
            .load_plugin("reentrant", plugin_context())
            .await
    });

    timeout(Duration::from_secs(1), async {
        while loaded.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first load should enter on_load");

    let second = timeout(
        Duration::from_secs(1),
        manager.load_plugin("reentrant", plugin_context()),
    )
    .await
    .expect("second load should not timeout");

    match second {
        Err(PluginLoadError::Loading(id)) => assert_eq!(id, "reentrant"),
        other => panic!("expected Loading error, got {:?}", other),
    }
    assert_eq!(loaded.load(Ordering::SeqCst), 1);

    release.notify_waiters();
    let first = timeout(Duration::from_secs(1), first_handle)
        .await
        .expect("first load task should not timeout")
        .expect("first load task should join");
    first.expect("first load should succeed");

    assert!(manager.is_loaded("reentrant"));
    assert_eq!(loaded.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_manager_clears_loading_state_after_hook_error() {
    let manager = PluginManager::new();
    let attempts = Arc::new(AtomicUsize::new(0));

    manager
        .register_plugin(FailingPlugin {
            id: "failing".to_string(),
            name: "Failing Plugin".to_string(),
            attempts: Arc::clone(&attempts),
            reason: "intentional failure".to_string(),
        })
        .expect("register should succeed");

    let first = manager
        .load_plugin("failing", plugin_context())
        .await
        .expect_err("first load should fail");
    match &first {
        PluginLoadError::Hook { id, reason } => {
            assert_eq!(id, "failing");
            assert_eq!(reason, "intentional failure");
        }
        other => panic!("expected Hook error, got {:?}", other),
    }
    assert_eq!(
        first.to_string(),
        "plugin 'failing' load hook failed: intentional failure"
    );
    assert!(!manager.is_loaded("failing"));

    let second = manager
        .load_plugin("failing", plugin_context())
        .await
        .expect_err("second load should fail again");
    match second {
        PluginLoadError::Hook { id, reason } => {
            assert_eq!(id, "failing");
            assert_eq!(reason, "intentional failure");
        }
        other => panic!("expected Hook error, got {:?}", other),
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

async fn start_http_capture_server() -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("capture server should bind");
    let address = listener
        .local_addr()
        .expect("capture server should expose local addr");

    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("request should arrive");
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        let mut expected_len: Option<usize> = None;

        loop {
            let read = stream.read(&mut buffer).await.expect("request should read");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if expected_len.is_none() {
                expected_len = http_request_total_length(request.as_slice());
            }
            if expected_len.is_some_and(|len| request.len() >= len) {
                break;
            }
        }

        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            )
            .await
            .expect("response should write");
        String::from_utf8(request).expect("request should stay utf-8")
    });

    (format!("http://{address}/"), task)
}

fn http_request_total_length(request: &[u8]) -> Option<usize> {
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?
        + 4;
    let headers = std::str::from_utf8(&request[..header_end]).ok()?;
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("Content-Length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    Some(header_end + content_length)
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn create() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let unique = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "liteyuki-rs-plugin-test-{nanos}-{}-{unique}",
            std::process::id()
        ));
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

fn python_command_available() -> bool {
    ["python", "python3", "py"].iter().any(|command| {
        Command::new(command)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    })
}
