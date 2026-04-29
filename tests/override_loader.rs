use std::fs;
use std::path::PathBuf;

use liteyukibot_core::{PluginRuntimeKind, PluginType, discover_plugin_manifests_in_dirs};
use serde_json::{Value, json};

fn temp_root(name: &str) -> PathBuf {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    std::env::temp_dir().join(format!("rsliteyukibot-{name}-{unique}"))
}

fn expected_plugin_config_path(plugin_id: &str) -> String {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".liteyuki")
        .join("configs")
        .join("plugins")
        .join(format!("{plugin_id}.json"))
        .display()
        .to_string()
}

#[test]
fn discover_astrbot_override_manifest_synthesizes_descriptor() {
    let root = temp_root("astrbot-override");
    let source_root = root.join("astrbot_plugin").join("hello_world");
    let manifest_root = root.join("manifests");
    fs::create_dir_all(&source_root).expect("source root should be created");
    fs::create_dir_all(&manifest_root).expect("manifest root should be created");
    fs::write(
        source_root.join("metadata.yaml"),
        "name: hello_world\ndisplay_name: Hello World\ndesc: greeting\nauthor: tester\nversion: 1.2.3\nrepo: https://example.com/repo\nsupport_platforms:\n  - qq\n",
    )
    .expect("metadata should be written");
    fs::write(source_root.join("main.py"), "class Hello: pass\n").expect("main should exist");
    fs::write(
        source_root.join("_conf_schema.json"),
        "{\"token\":{\"type\":\"string\"}}",
    )
    .expect("schema should exist");
    fs::write(
        manifest_root.join("hello.override.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "pluginId": "astrbot-hello",
            "source": {
                "kind": "astrbot",
                "path": "astrbot_plugin/hello_world"
            }
        }))
        .expect("override should serialize"),
    )
    .expect("override should be written");

    let manifests =
        discover_plugin_manifests_in_dirs([root.as_path()]).expect("discovery should succeed");
    assert_eq!(manifests.len(), 1);
    let descriptor = &manifests[0].descriptor;
    assert_eq!(descriptor.metadata.id, "astrbot-hello");
    assert_eq!(descriptor.metadata.name, "Hello World");
    assert_eq!(descriptor.metadata.description, "greeting");
    assert_eq!(descriptor.runtime.kind, PluginRuntimeKind::Python);
    assert_eq!(descriptor.runtime.entrypoint, "main");
    assert_eq!(
        descriptor
            .metadata
            .extra
            .get("sourceFamily")
            .and_then(|value| value.as_str()),
        Some("astrbot")
    );
    assert_eq!(
        descriptor
            .metadata
            .extra
            .get("adapterFamily")
            .and_then(|value| value.as_str()),
        Some("astrbot_python_bridge")
    );
    assert!(
        descriptor
            .permissions
            .iter()
            .any(|value| value == "config.read")
    );
    assert!(
        descriptor
            .permissions
            .iter()
            .any(|value| value == "config.write")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn discover_astrbot_override_without_plugin_id_uses_derived_config_path() {
    let root = temp_root("astrbot-derived-config");
    let source_root = root.join("astrbot_plugin").join("hello_world");
    let manifest_root = root.join("manifests");
    fs::create_dir_all(&source_root).expect("source root should be created");
    fs::create_dir_all(&manifest_root).expect("manifest root should be created");
    fs::write(
        source_root.join("metadata.yaml"),
        "name: hello_world\ndisplay_name: Hello World\ndesc: greeting\n",
    )
    .expect("metadata should be written");
    fs::write(source_root.join("main.py"), "class Hello: pass\n").expect("main should exist");
    fs::write(
        source_root.join("_conf_schema.json"),
        "{\"token\":{\"type\":\"string\"}}",
    )
    .expect("schema should exist");
    fs::write(
        manifest_root.join("hello.override.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "source": {
                "kind": "astrbot",
                "path": "astrbot_plugin/hello_world"
            }
        }))
        .expect("override should serialize"),
    )
    .expect("override should be written");

    let manifests =
        discover_plugin_manifests_in_dirs([root.as_path()]).expect("discovery should succeed");
    assert_eq!(manifests.len(), 1);
    let descriptor = &manifests[0].descriptor;
    assert_eq!(descriptor.metadata.id, "hello-world");
    assert_eq!(descriptor.metadata.name, "Hello World");
    assert!(
        descriptor
            .runtime
            .options
            .get("config_path")
            .and_then(Value::as_str)
            == Some(expected_plugin_config_path("hello-world").as_str()),
        "unexpected config_path: {:?}",
        descriptor
            .runtime
            .options
            .get("config_path")
            .and_then(Value::as_str)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn discover_liteyuki_override_manifest_extracts_plugin_meta() {
    let root = temp_root("liteyuki-override");
    let source_root = root.join("liteyukibot_plugin").join("hello_weather");
    let manifest_root = root.join("manifests");
    fs::create_dir_all(&source_root).expect("source root should be created");
    fs::create_dir_all(&manifest_root).expect("manifest root should be created");
    fs::write(
        source_root.join("__init__.py"),
        "from liteyuki.plugin import PluginMetadata, PluginType\n__plugin_meta__ = PluginMetadata(\n    name=\"Weather\",\n    description=\"weather lookup\",\n    type=PluginType.APPLICATION,\n    author=\"tester\",\n    homepage=\"https://example.com/weather\",\n)\n",
    )
    .expect("metadata should be written");
    fs::write(
        manifest_root.join("weather.override.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "source": {
                "kind": "liteyuki_py",
                "path": "liteyukibot_plugin/hello_weather"
            }
        }))
        .expect("override should serialize"),
    )
    .expect("override should be written");

    let manifests =
        discover_plugin_manifests_in_dirs([root.as_path()]).expect("discovery should succeed");
    assert_eq!(manifests.len(), 1);
    let descriptor = &manifests[0].descriptor;
    assert_eq!(descriptor.metadata.name, "Weather");
    assert_eq!(descriptor.metadata.description, "weather lookup");
    assert_eq!(descriptor.metadata.plugin_type, PluginType::Application);
    assert_eq!(descriptor.runtime.entrypoint, "hello_weather");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn discover_neomofox_override_manifest_extracts_manifest_metadata() {
    let root = temp_root("neomofox-override");
    let source_root = root.join("neomofox_plugin").join("emoji_sender");
    let manifest_root = root.join("manifests");
    fs::create_dir_all(&source_root).expect("source root should be created");
    fs::create_dir_all(&manifest_root).expect("manifest root should be created");
    fs::write(source_root.join("__init__.py"), "").expect("package should exist");
    fs::write(
        source_root.join("plugin.py"),
        "from src.core.components import BasePlugin, register_plugin\n@register_plugin\nclass EmojiSenderPlugin(BasePlugin):\n    plugin_name = 'emoji_sender'\n    plugin_description = 'emoji sender'\n    plugin_version = '1.0.0'\n",
    )
    .expect("entrypoint should exist");
    fs::write(
        source_root.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "name": "emoji_sender",
            "version": "1.0.0",
            "description": "send emoji memes",
            "author": "MoFox Team",
            "entry_point": "plugin.py",
            "min_core_version": "1.0.0",
            "python_dependencies": ["pillow"],
            "include": [
                {
                    "component_type": "service",
                    "component_name": "emoji_sender",
                    "enabled": true
                }
            ]
        }))
        .expect("manifest should serialize"),
    )
    .expect("manifest should be written");
    fs::write(
        manifest_root.join("emoji.override.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "source": {
                "kind": "neomofox",
                "path": "neomofox_plugin/emoji_sender"
            }
        }))
        .expect("override should serialize"),
    )
    .expect("override should be written");

    let manifests =
        discover_plugin_manifests_in_dirs([root.as_path()]).expect("discovery should succeed");
    assert_eq!(manifests.len(), 1);
    let descriptor = &manifests[0].descriptor;
    assert_eq!(descriptor.metadata.id, "emoji-sender");
    assert_eq!(descriptor.metadata.name, "emoji_sender");
    assert_eq!(descriptor.metadata.description, "send emoji memes");
    assert_eq!(descriptor.runtime.kind, PluginRuntimeKind::Python);
    assert_eq!(descriptor.runtime.entrypoint, "emoji_sender.plugin");
    assert_eq!(
        descriptor
            .runtime
            .options
            .get("compat_family")
            .and_then(Value::as_str),
        Some("neomofox")
    );
    assert_eq!(
        descriptor
            .metadata
            .extra
            .get("sourceFamily")
            .and_then(|value| value.as_str()),
        Some("neomofox")
    );
    assert_eq!(
        descriptor
            .metadata
            .extra
            .get("adapterFamily")
            .and_then(|value| value.as_str()),
        Some("neomofox_python_bridge")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn discover_neomofox_override_keeps_file_path_entrypoint_segments() {
    let root = temp_root("neomofox-hyphen-entrypoint");
    let source_root = root.join("neomofox_plugin").join("my-plugin");
    let manifest_root = root.join("manifests");
    fs::create_dir_all(source_root.join("src")).expect("source root should be created");
    fs::create_dir_all(&manifest_root).expect("manifest root should be created");
    fs::write(source_root.join("__init__.py"), "").expect("package should exist");
    fs::write(
        source_root.join("src").join("plugin.py"),
        "from src.core.components import BasePlugin, register_plugin\n@register_plugin\nclass MyPlugin(BasePlugin):\n    plugin_name = 'my_plugin'\n    def get_components(self):\n        return []\n",
    )
    .expect("entrypoint should exist");
    fs::write(
        source_root.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "name": "my_plugin",
            "version": "1.0.0",
            "entry_point": "src/plugin.py"
        }))
        .expect("manifest should serialize"),
    )
    .expect("manifest should be written");
    fs::write(
        manifest_root.join("my-plugin.override.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "source": {
                "kind": "neomofox",
                "path": "neomofox_plugin/my-plugin"
            }
        }))
        .expect("override should serialize"),
    )
    .expect("override should be written");

    let manifests =
        discover_plugin_manifests_in_dirs([root.as_path()]).expect("discovery should succeed");
    assert_eq!(manifests.len(), 1);
    assert_eq!(
        manifests[0].descriptor.runtime.entrypoint,
        "my-plugin.src.plugin"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_native_manifest_wins_when_override_uses_same_plugin_id() {
    let root = temp_root("native-priority");
    let native_root = root.join("demo_plugin");
    let manifest_root = root.join("manifests");
    fs::create_dir_all(&native_root).expect("native root should be created");
    fs::create_dir_all(&manifest_root).expect("manifest root should be created");
    fs::write(
        native_root.join("plugin.json"),
        serde_json::to_vec_pretty(&json!({
            "id": "demo-plugin",
            "name": "Native Demo",
            "runtime": {
                "kind": "native",
                "entrypoint": "demo"
            }
        }))
        .expect("manifest should serialize"),
    )
    .expect("plugin manifest should be written");
    fs::create_dir_all(root.join("astrbot_plugin").join("demo")).expect("source root");
    fs::write(
        root.join("astrbot_plugin")
            .join("demo")
            .join("metadata.yaml"),
        "name: demo\ndesc: fallback\n",
    )
    .expect("metadata should be written");
    fs::write(
        root.join("astrbot_plugin").join("demo").join("main.py"),
        "class Demo: pass\n",
    )
    .expect("main should be written");
    fs::write(
        manifest_root.join("demo.override.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "pluginId": "demo-plugin",
            "source": {
                "kind": "astrbot",
                "path": "astrbot_plugin/demo"
            }
        }))
        .expect("override should serialize"),
    )
    .expect("override should be written");

    let manifests = discover_plugin_manifests_in_dirs([native_root.as_path(), root.as_path()])
        .expect("discovery should succeed");
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].descriptor.metadata.name, "Native Demo");

    let _ = fs::remove_dir_all(root);
}
