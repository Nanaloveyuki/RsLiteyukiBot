use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::config_paths::resolve_user_config_dir;
use crate::plugin::loader::{
    PluginManifest, PluginManifestError, normalize_manifest_permissions, normalize_plugin_id,
    validate_manifest_commands,
};
use crate::plugin::{
    PluginDescriptor, PluginMetadata, PluginRuntimeSpec, PluginSdkSpec, PluginType,
};

use super::family::build_family_seed;
use super::model::*;

pub fn discover_plugin_manifests_in_dirs<I, P>(
    dirs: I,
) -> Result<Vec<PluginManifest>, PluginManifestError>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut manifests = Vec::new();
    let mut seen_ids = HashSet::new();
    for dir in dirs {
        let dir = dir.as_ref();
        if !dir.exists() {
            continue;
        }

        discover_native_manifests_in_dir(dir, &mut manifests, &mut seen_ids)?;
        discover_override_manifests_in_dir(dir, &mut manifests, &mut seen_ids)?;
    }
    Ok(manifests)
}

fn discover_native_manifests_in_dir(
    dir: &Path,
    manifests: &mut Vec<PluginManifest>,
    seen_ids: &mut HashSet<String>,
) -> Result<(), PluginManifestError> {
    if dir.is_file() {
        if dir.file_name().and_then(|name| name.to_str()) == Some("plugin.json") {
            push_unique_manifest(
                crate::plugin::PluginManifestLoader::load_manifest(dir)?,
                manifests,
                seen_ids,
            );
        }
        return Ok(());
    }

    let root_manifest = dir.join("plugin.json");
    if root_manifest.is_file() {
        push_unique_manifest(
            crate::plugin::PluginManifestLoader::load_manifest(&root_manifest)?,
            manifests,
            seen_ids,
        );
    }

    let entries = std::fs::read_dir(dir).map_err(|err| {
        PluginManifestError::Io(format!("read_dir failed for {}: {}", dir.display(), err))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            PluginManifestError::Io(format!(
                "read_dir entry failed for {}: {}",
                dir.display(),
                err
            ))
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("plugin.json");
        if manifest.is_file() {
            push_unique_manifest(
                crate::plugin::PluginManifestLoader::load_manifest(&manifest)?,
                manifests,
                seen_ids,
            );
        }
    }
    Ok(())
}

fn discover_override_manifests_in_dir(
    dir: &Path,
    manifests: &mut Vec<PluginManifest>,
    seen_ids: &mut HashSet<String>,
) -> Result<(), PluginManifestError> {
    if !dir.is_dir() {
        return Ok(());
    }
    let manifest_dir = dir.join(OVERRIDE_MANIFEST_DIR);
    if !manifest_dir.is_dir() {
        return Ok(());
    }

    let mut files = std::fs::read_dir(&manifest_dir)
        .map_err(|err| {
            PluginManifestError::Io(format!(
                "read_dir failed for {}: {}",
                manifest_dir.display(),
                err
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            PluginManifestError::Io(format!(
                "read_dir entry failed for {}: {}",
                manifest_dir.display(),
                err
            ))
        })?;
    files.sort_by_key(|entry| entry.file_name());

    for entry in files {
        let path = entry.path();
        let Some(filename) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !path.is_file() || !filename.ends_with(OVERRIDE_MANIFEST_SUFFIX) {
            continue;
        }
        let manifest = load_override_manifest(dir, &path)?;
        push_unique_manifest(manifest, manifests, seen_ids);
    }
    Ok(())
}

fn push_unique_manifest(
    manifest: PluginManifest,
    manifests: &mut Vec<PluginManifest>,
    seen_ids: &mut HashSet<String>,
) {
    let id = manifest.descriptor.metadata.id.clone();
    if seen_ids.insert(id) {
        manifests.push(manifest);
    }
}

fn load_override_manifest(
    plugin_root: &Path,
    override_path: &Path,
) -> Result<PluginManifest, PluginManifestError> {
    let content = std::fs::read_to_string(override_path).map_err(|err| {
        PluginManifestError::Io(format!(
            "read override manifest failed for {}: {}",
            override_path.display(),
            err
        ))
    })?;
    let raw: SourceOverrideManifestDoc = serde_json::from_str(content.as_str()).map_err(|err| {
        PluginManifestError::Parse(format!(
            "parse override manifest failed for {}: {}",
            override_path.display(),
            err
        ))
    })?;
    if raw.version != 1 {
        return Err(PluginManifestError::Parse(format!(
            "unsupported override manifest version {} in {}",
            raw.version,
            override_path.display()
        )));
    }

    let source_root =
        resolve_source_root(plugin_root, raw.source.path.as_str()).map_err(|err| {
            PluginManifestError::Parse(format!(
                "invalid override manifest in {}: {}",
                override_path.display(),
                err
            ))
        })?;
    if !source_root.is_dir() {
        return Err(PluginManifestError::Parse(format!(
            "invalid override manifest in {}: source path {} is not a directory",
            override_path.display(),
            source_root.display()
        )));
    }

    let requested_id = raw
        .plugin_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let seed = build_family_seed(
        raw.source.kind,
        requested_id.as_deref().unwrap_or(""),
        &source_root,
    )
    .map_err(|err| {
        PluginManifestError::Parse(format!(
            "invalid override manifest in {}: {}",
            override_path.display(),
            err
        ))
    })?;
    let plugin_id = requested_id
        .or_else(|| seed.plugin_id_hint.clone())
        .or_else(|| {
            source_root
                .file_name()
                .and_then(|value| value.to_str())
                .map(normalize_plugin_id)
                .filter(|value| !value.trim().is_empty())
        })
        .ok_or_else(|| {
            PluginManifestError::Parse(format!(
                "invalid override manifest in {}: failed to derive plugin id",
                override_path.display()
            ))
        })?;

    let host = raw.host.unwrap_or_default();
    let mut runtime = merge_runtime(seed.runtime, host.runtime.unwrap_or_default());
    if raw.source.kind == SourcePluginFamily::Astrbot
        && source_root.join("_conf_schema.json").is_file()
        && runtime
            .options
            .get("config_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        runtime.options.insert(
            "config_path".to_string(),
            Value::String(
                default_plugin_config_path(plugin_id.as_str())
                    .display()
                    .to_string(),
            ),
        );
    }
    let sdk = merge_sdk(seed.sdk, host.sdk.unwrap_or_default());
    let mut commands = host.commands.unwrap_or(seed.commands);
    validate_manifest_commands(commands.as_mut_slice(), override_path)?;

    let permissions = normalize_manifest_permissions(
        host.permissions.unwrap_or(seed.permissions).as_slice(),
        override_path,
    )?;
    let mut extra = seed.extra;
    if let Some(host_extra) = host.extra {
        extra.extend(host_extra);
    }
    inject_source_extra(
        &mut extra,
        raw.source.kind,
        source_root
            .strip_prefix(plugin_root)
            .unwrap_or(&source_root),
        override_path
            .strip_prefix(plugin_root)
            .unwrap_or(override_path),
        seed.source_manifest_name.as_deref(),
    );
    if !raw.source.metadata_files.is_empty() {
        extra.insert(
            "metadataFiles".to_string(),
            Value::Array(
                raw.source
                    .metadata_files
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if let Some(config_strategy) = raw.source.config_strategy.and_then(|doc| doc.kind) {
        extra.insert("configStrategy".to_string(), Value::String(config_strategy));
    }

    let name = host
        .name
        .filter(|value| !value.trim().is_empty())
        .or(seed.name)
        .unwrap_or_else(|| plugin_id.clone());
    let descriptor = PluginDescriptor {
        metadata: PluginMetadata {
            id: plugin_id.clone(),
            name,
            description: host
                .description
                .unwrap_or_else(|| seed.description.unwrap_or_default()),
            plugin_type: host
                .plugin_type
                .or(seed.plugin_type)
                .unwrap_or(PluginType::Unclassified),
            author: host
                .author
                .unwrap_or_else(|| seed.author.unwrap_or_default()),
            homepage: host
                .homepage
                .unwrap_or_else(|| seed.homepage.unwrap_or_default()),
            extra,
        },
        runtime,
        sdk,
        permissions,
        commands,
        manifest_path: Some(source_root.join(SYNTHETIC_MANIFEST_FILENAME)),
    };
    let synthetic_path = source_root.join(SYNTHETIC_MANIFEST_FILENAME);
    Ok(PluginManifest {
        descriptor,
        path: synthetic_path,
    })
}

fn merge_runtime(
    mut base: PluginRuntimeSpec,
    override_doc: RuntimeOverrideDoc,
) -> PluginRuntimeSpec {
    if let Some(kind) = override_doc.kind {
        base.kind = kind;
    }
    if let Some(entrypoint) = override_doc.entrypoint {
        base.entrypoint = entrypoint;
    }
    if let Some(module) = override_doc.module {
        base.module = module;
    }
    if let Some(abi) = override_doc.abi {
        base.abi = abi;
    }
    if let Some(min_version) = override_doc.min_version {
        base.min_version = min_version;
    }
    if let Some(options) = override_doc.options {
        base.options.extend(options);
    }
    base
}

fn merge_sdk(mut base: PluginSdkSpec, override_doc: SdkOverrideDoc) -> PluginSdkSpec {
    if let Some(api_version) = override_doc.api_version {
        base.api_version = api_version;
    }
    if let Some(min_host_version) = override_doc.min_host_version {
        base.min_host_version = min_host_version;
    }
    if let Some(options) = override_doc.options {
        base.options.extend(options);
    }
    base
}

fn inject_source_extra(
    extra: &mut HashMap<String, Value>,
    family: SourcePluginFamily,
    source_path: &Path,
    override_path: &Path,
    source_manifest_name: Option<&str>,
) {
    let (adapter_family, compat_level) = match family {
        SourcePluginFamily::Native => (SourceAdapterFamily::Native, SourceCompatLevel::Native),
        SourcePluginFamily::LiteyukiPy => (
            SourceAdapterFamily::LiteyukiPythonBridge,
            SourceCompatLevel::Bridged,
        ),
        SourcePluginFamily::Astrbot => (
            SourceAdapterFamily::AstrbotPythonBridge,
            SourceCompatLevel::Bridged,
        ),
        SourcePluginFamily::Neomofox => (
            SourceAdapterFamily::NeomofoxPythonBridge,
            SourceCompatLevel::Bridged,
        ),
        SourcePluginFamily::Nonebot => (
            SourceAdapterFamily::NonebotExternal,
            SourceCompatLevel::MetadataOnly,
        ),
    };
    extra.insert(
        EXTRA_SOURCE_FAMILY.to_string(),
        serde_json::to_value(family).unwrap_or_else(|_| Value::String("native".to_string())),
    );
    extra.insert(
        EXTRA_ADAPTER_FAMILY.to_string(),
        serde_json::to_value(adapter_family)
            .unwrap_or_else(|_| Value::String("native".to_string())),
    );
    extra.insert(
        EXTRA_COMPAT_LEVEL.to_string(),
        serde_json::to_value(compat_level).unwrap_or_else(|_| Value::String("native".to_string())),
    );
    extra.insert(
        EXTRA_SOURCE_PATH.to_string(),
        Value::String(path_to_forward_slashes(source_path)),
    );
    extra.insert(
        EXTRA_OVERRIDE_MANIFEST_PATH.to_string(),
        Value::String(path_to_forward_slashes(override_path)),
    );
    if let Some(name) = source_manifest_name {
        extra.insert(
            EXTRA_SOURCE_MANIFEST.to_string(),
            Value::String(name.to_string()),
        );
        extra.insert(
            EXTRA_SOURCE_MANIFEST_PATH.to_string(),
            Value::String(path_to_forward_slashes(&source_path.join(name))),
        );
    }
}

fn resolve_source_root(plugin_root: &Path, raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_start_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err("source.path should not be empty".to_string());
    }

    let mut relative = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(segment) => relative.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err("source.path escapes plugin root".to_string());
            }
        }
    }
    Ok(plugin_root.join(relative))
}

fn path_to_forward_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn default_plugin_config_path(plugin_id: &str) -> PathBuf {
    resolve_user_config_dir()
        .join("plugins")
        .join(format!("{plugin_id}.json"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::config_paths::resolve_user_config_dir;
    use serde_json::Value;
    use serde_json::json;

    use super::discover_plugin_manifests_in_dirs;

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
        assert_eq!(descriptor.runtime.kind, crate::PluginRuntimeKind::Python);
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
        assert_eq!(
            descriptor
                .runtime
                .options
                .get("config_path")
                .and_then(Value::as_str),
            Some(
                resolve_user_config_dir()
                    .join("plugins")
                    .join("hello-world.json")
                    .display()
                    .to_string()
                    .as_str()
            )
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
        assert_eq!(
            descriptor.metadata.plugin_type,
            crate::PluginType::Application
        );
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
        assert_eq!(descriptor.runtime.kind, crate::PluginRuntimeKind::Python);
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
}
