use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use liteyukibot_core::{
    AdapterManager, ChannelRegistry, LifecycleContext, Plugin, PluginAbiMethod, PluginContext,
    PluginHostBridge, PluginLoadError, PluginLoadState, PluginManager, PluginMetadata,
    PluginRuntimeKind, PluginSdk, PluginType, RuntimeTarget, SessionRouter, SharedStore,
};
use liteyukibot_core::{BotEvent, Logger, LoggerConfig};
use liteyukibot_core::{RuntimeCapabilities, RuntimeFlavor};
use serde_json::json;
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};

struct CountingPlugin {
    id: String,
    name: String,
    loaded: Arc<AtomicUsize>,
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
    assert_eq!(loaded[0].load_plan.runtime_kind, PluginRuntimeKind::Python);
    assert_eq!(loaded[0].load_plan.state, PluginLoadState::Deferred);
    assert_eq!(
        loaded[0].load_plan.contract.abi_name,
        "liteyuki-python-bridge"
    );
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

    let updated_config =
        std::fs::read_to_string(config_path).expect("updated config should stay readable");
    assert!(updated_config.contains("value: 2"));
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
        plugin_dir.join("liteecho.py"),
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
  "runtime": {
    "kind": "python",
    "entrypoint": "liteecho",
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

    context.sdk.dispatch_event(
        &BotEvent::new(
            42,
            "adapter.inbound",
            json!({
                "_adapter_id": "missing-adapter",
                "message_type": "group",
                "group_id": "10001",
                "user_id": "20001",
                "raw_message": "liteecho hello from legacy"
            }),
        ),
        &context.logger,
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
