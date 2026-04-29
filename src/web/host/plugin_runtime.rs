use super::*;

fn localized_text(raw: &str) -> String {
    let snapshot = current_i18n_snapshot();
    snapshot
        .messages
        .get(raw)
        .cloned()
        .unwrap_or_else(|| raw.to_string())
}

fn plugin_extra_string(extra: &Map<String, Value>, key: &str) -> Option<String> {
    extra
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn plugin_can_read_config(descriptor: &crate::PluginDescriptor) -> bool {
    descriptor
        .permissions
        .iter()
        .any(|permission| permission == "config.read")
}

pub(super) fn plugin_can_write_config(descriptor: &crate::PluginDescriptor) -> bool {
    descriptor
        .permissions
        .iter()
        .any(|permission| permission == "config.write")
}

pub(super) fn plugin_declared_config_path(descriptor: &crate::PluginDescriptor) -> Option<&str> {
    descriptor
        .runtime
        .options
        .get("config_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

fn descriptor_has_config(descriptor: &crate::PluginDescriptor) -> bool {
    plugin_declared_config_path(descriptor).is_some()
        && (plugin_can_read_config(descriptor) || plugin_can_write_config(descriptor))
}

fn plugin_has_config(entry: &crate::PluginCatalogEntry) -> bool {
    descriptor_has_config(&entry.descriptor)
}

fn plugin_extension_pages(entry: &crate::PluginCatalogEntry) -> Vec<Value> {
    let pages = entry
        .descriptor
        .metadata
        .extra
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let plugin_name = localized_text(entry.descriptor.metadata.name.as_str());

    pages
        .into_iter()
        .filter_map(|page| {
            let object = page.as_object()?;
            let path = object
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?
                .to_string();
            let title = object
                .get("title")
                .and_then(Value::as_str)
                .map(localized_text)
                .unwrap_or_else(|| path.clone());
            let mut value = Map::new();
            value.insert(
                "pluginId".to_string(),
                Value::String(entry.descriptor.metadata.id.clone()),
            );
            value.insert("pluginName".to_string(), Value::String(plugin_name.clone()));
            value.insert("path".to_string(), Value::String(path));
            value.insert("title".to_string(), Value::String(title));
            if let Some(icon) = object
                .get("icon")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|icon| !icon.is_empty())
            {
                value.insert("icon".to_string(), Value::String(icon.to_string()));
            }
            if let Some(description) = object
                .get("description")
                .and_then(Value::as_str)
                .map(localized_text)
            {
                value.insert("description".to_string(), Value::String(description));
            }
            Some(Value::Object(value))
        })
        .collect()
}

pub(super) fn plugin_declared_page_paths(descriptor: &crate::PluginDescriptor) -> Vec<String> {
    descriptor
        .metadata
        .extra
        .get("pages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|page| {
            page.as_object()?
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .collect()
}

pub(super) fn infer_plugin_config_schema(config: &Map<String, Value>) -> Vec<Value> {
    let mut fields = config
        .iter()
        .map(|(key, value)| {
            let mut field = Map::new();
            field.insert("key".to_string(), Value::String(key.clone()));
            field.insert("label".to_string(), Value::String(key.clone()));
            match value {
                Value::Bool(current) => {
                    field.insert("type".to_string(), Value::String("boolean".to_string()));
                    field.insert("default".to_string(), Value::Bool(*current));
                }
                Value::Number(current) => {
                    field.insert("type".to_string(), Value::String("number".to_string()));
                    field.insert("default".to_string(), Value::Number(current.clone()));
                }
                Value::String(current) => {
                    field.insert("type".to_string(), Value::String("string".to_string()));
                    field.insert("default".to_string(), Value::String(current.clone()));
                }
                Value::Null => {
                    field.insert("type".to_string(), Value::String("string".to_string()));
                    field.insert("default".to_string(), Value::String(String::new()));
                }
                complex => {
                    field.insert("type".to_string(), Value::String("text".to_string()));
                    field.insert(
                        "default".to_string(),
                        Value::String(
                            serde_json::to_string_pretty(complex)
                                .unwrap_or_else(|_| complex.to_string()),
                        ),
                    );
                    field.insert(
                        "description".to_string(),
                        Value::String(
                            "Complex values are currently read-only in WebUI.".to_string(),
                        ),
                    );
                }
            }
            Value::Object(field)
        })
        .collect::<Vec<_>>();
    fields.sort_by(|left, right| {
        left["key"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["key"].as_str().unwrap_or_default())
    });
    fields
}

fn discover_plugin_descriptor(plugin_id: &str) -> Option<crate::PluginDescriptor> {
    let plugin_dirs = resolve_builtin_plugin_dirs();
    let manifests = discover_plugin_manifests_in_dirs(plugin_dirs.iter()).ok()?;
    manifests
        .into_iter()
        .find(|manifest| manifest.descriptor.metadata.id == plugin_id)
        .map(|manifest| manifest.descriptor)
}

pub(super) fn resolve_plugin_descriptor(
    runtime_host: Option<&EmbeddedAppHost>,
    plugin_id: &str,
) -> Option<crate::PluginDescriptor> {
    runtime_host
        .and_then(|host| {
            run_async_for_web_host(host.plugin_catalog_snapshot())
                .entries
                .into_iter()
                .find(|entry| entry.descriptor.metadata.id == plugin_id)
                .map(|entry| entry.descriptor)
        })
        .or_else(|| discover_plugin_descriptor(plugin_id))
}

pub(super) fn build_runtime_plugin_payload(
    runtime_host: &EmbeddedAppHost,
    snapshot: AppHostPluginCatalogSnapshot,
) -> Value {
    let disabled = snapshot
        .disabled_plugin_ids
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let capability_snapshots =
        run_async_for_web_host(runtime_host.all_plugin_capability_snapshots())
            .unwrap_or_default()
            .into_iter()
            .map(|snapshot| (snapshot.plugin_id.clone(), snapshot))
            .collect::<HashMap<_, _>>();
    let mut extension_pages = Vec::new();
    let mut plugins = Vec::new();

    for entry in snapshot.entries {
        let metadata = &entry.descriptor.metadata;
        let extra = Map::from_iter(
            metadata
                .extra
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        let pages = plugin_extension_pages(&entry);
        let capability_snapshot = capability_snapshots.get(metadata.id.as_str());
        let compat_kind = descriptor_compat_kind(&entry.descriptor, capability_snapshot);
        let source_kind = descriptor_source_kind(&entry.descriptor, capability_snapshot);
        let status = if disabled.contains(metadata.id.as_str()) {
            "disabled"
        } else if entry.loaded && entry.load_state != Some(PluginLoadState::Deferred) {
            "active"
        } else {
            "stopped"
        };
        let mut plugin = Map::new();
        plugin.insert(
            "name".to_string(),
            Value::String(localized_text(metadata.name.as_str())),
        );
        plugin.insert("id".to_string(), Value::String(metadata.id.clone()));
        plugin.insert(
            "version".to_string(),
            Value::String(
                plugin_extra_string(&extra, "version").unwrap_or_else(|| "builtin".to_string()),
            ),
        );
        plugin.insert(
            "description".to_string(),
            Value::String(localized_text(metadata.description.as_str())),
        );
        plugin.insert("author".to_string(), Value::String(metadata.author.clone()));
        plugin.insert(
            "runtimeKind".to_string(),
            serde_json::to_value(entry.descriptor.runtime.kind)
                .unwrap_or_else(|_| Value::String("native".to_string())),
        );
        plugin.insert(
            "pluginType".to_string(),
            serde_json::to_value(metadata.plugin_type)
                .unwrap_or_else(|_| Value::String("unclassified".to_string())),
        );
        plugin.insert(
            "sourceKind".to_string(),
            Value::String(source_kind.to_string()),
        );
        plugin.insert(
            "compatKind".to_string(),
            Value::String(compat_kind.to_string()),
        );
        if let Some(source_family) = descriptor_source_family(&entry.descriptor) {
            plugin.insert(
                "sourceFamily".to_string(),
                serde_json::to_value(source_family)
                    .unwrap_or_else(|_| Value::String("native".to_string())),
            );
        }
        if let Some(adapter_family) = descriptor_adapter_family(&entry.descriptor) {
            plugin.insert(
                "adapterFamily".to_string(),
                serde_json::to_value(adapter_family)
                    .unwrap_or_else(|_| Value::String("native".to_string())),
            );
        }
        if let Some(compat_level) = descriptor_compat_level(&entry.descriptor) {
            plugin.insert(
                "compatLevel".to_string(),
                serde_json::to_value(compat_level)
                    .unwrap_or_else(|_| Value::String("native".to_string())),
            );
        }
        if let Some(source_path) = descriptor_family_value(&entry.descriptor, "sourcePath") {
            plugin.insert("sourcePath".to_string(), source_path);
        }
        plugin.insert("status".to_string(), Value::String(status.to_string()));
        plugin.insert(
            "hasConfig".to_string(),
            Value::Bool(plugin_has_config(&entry)),
        );
        plugin.insert("hasPages".to_string(), Value::Bool(!pages.is_empty()));
        plugin.insert(
            "hasCapabilities".to_string(),
            plugin_capability_flags(capability_snapshot),
        );
        if !metadata.homepage.trim().is_empty() {
            plugin.insert(
                "homepage".to_string(),
                Value::String(metadata.homepage.clone()),
            );
        }
        if let Some(repository) = plugin_extra_string(&extra, "repository") {
            plugin.insert("repository".to_string(), Value::String(repository));
        }
        if let Some(icon) = plugin_extra_string(&extra, "icon") {
            plugin.insert("icon".to_string(), Value::String(icon));
        }
        extension_pages.extend(pages);
        plugins.push(Value::Object(plugin));
    }

    Value::Object(Map::from_iter([
        ("plugins".to_string(), Value::Array(plugins)),
        ("pluginManagerNotFound".to_string(), Value::Bool(false)),
        ("extensionPages".to_string(), Value::Array(extension_pages)),
    ]))
}

pub(super) fn discover_plugins() -> Vec<Value> {
    let (doc, _) = load_app_config_with_warnings(false);
    let disabled = resolve_disabled_plugins(&doc);
    let plugin_dirs = resolve_builtin_plugin_dirs();
    let Ok(manifests) = discover_plugin_manifests_in_dirs(plugin_dirs.iter()) else {
        return Vec::new();
    };

    manifests
        .into_iter()
        .map(|manifest| {
            let descriptor = manifest.descriptor;
            let id = descriptor.metadata.id.clone();
            let description = localized_text(descriptor.metadata.description.as_str());
            let name = localized_text(descriptor.metadata.name.as_str());
            let version = descriptor
                .metadata
                .extra
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("builtin")
                .to_string();
            let author = descriptor.metadata.author.clone();
            let homepage = descriptor.metadata.homepage.clone();
            let pages = plugin_declared_page_paths(&descriptor);
            let compat_kind = descriptor_compat_kind(&descriptor, None);
            let source_kind = descriptor_source_kind(&descriptor, None);
            let status = if disabled.iter().any(|entry| entry == &id) {
                "disabled"
            } else {
                "active"
            };
            Value::Object(Map::from_iter([
                ("name".to_string(), Value::String(name)),
                ("id".to_string(), Value::String(id.clone())),
                ("version".to_string(), Value::String(version)),
                ("description".to_string(), Value::String(description)),
                ("author".to_string(), Value::String(author)),
                (
                    "runtimeKind".to_string(),
                    serde_json::to_value(descriptor.runtime.kind)
                        .unwrap_or_else(|_| Value::String("native".to_string())),
                ),
                (
                    "pluginType".to_string(),
                    serde_json::to_value(descriptor.metadata.plugin_type)
                        .unwrap_or_else(|_| Value::String("unclassified".to_string())),
                ),
                (
                    "sourceKind".to_string(),
                    Value::String(source_kind.to_string()),
                ),
                (
                    "compatKind".to_string(),
                    Value::String(compat_kind.to_string()),
                ),
                (
                    "sourceFamily".to_string(),
                    descriptor_source_family(&descriptor)
                        .and_then(|value| serde_json::to_value(value).ok())
                        .unwrap_or(Value::Null),
                ),
                (
                    "adapterFamily".to_string(),
                    descriptor_adapter_family(&descriptor)
                        .and_then(|value| serde_json::to_value(value).ok())
                        .unwrap_or(Value::Null),
                ),
                (
                    "compatLevel".to_string(),
                    descriptor_compat_level(&descriptor)
                        .and_then(|value| serde_json::to_value(value).ok())
                        .unwrap_or(Value::Null),
                ),
                ("status".to_string(), Value::String(status.to_string())),
                (
                    "hasConfig".to_string(),
                    Value::Bool(descriptor_has_config(&descriptor)),
                ),
                ("hasPages".to_string(), Value::Bool(!pages.is_empty())),
                ("hasCapabilities".to_string(), plugin_capability_flags(None)),
                (
                    "sourcePath".to_string(),
                    descriptor_family_value(&descriptor, "sourcePath").unwrap_or(Value::Null),
                ),
                ("homepage".to_string(), Value::String(homepage)),
            ]))
        })
        .collect()
}

pub(super) fn plugin_capability_flags(snapshot: Option<&crate::PluginCapabilitySnapshot>) -> Value {
    let tools = snapshot.is_some_and(|snapshot| !snapshot.tools.is_empty());
    let web_apis = snapshot.is_some_and(|snapshot| !snapshot.web_apis.is_empty());
    let cron_jobs = snapshot.is_some_and(|snapshot| !snapshot.cron_jobs.is_empty());
    let tasks = snapshot.is_some_and(|snapshot| !snapshot.tasks.is_empty());

    Value::Object(Map::from_iter([
        (
            "any".to_string(),
            Value::Bool(tools || web_apis || cron_jobs || tasks),
        ),
        ("tools".to_string(), Value::Bool(tools)),
        ("webApis".to_string(), Value::Bool(web_apis)),
        ("cronJobs".to_string(), Value::Bool(cron_jobs)),
        ("tasks".to_string(), Value::Bool(tasks)),
    ]))
}

pub(super) fn update_disabled_plugins(plugin_id: &str, enable: bool) -> Result<(), String> {
    let Some(config_path) = resolve_app_config_path() else {
        return Err("app config path not found".to_string());
    };
    let (doc, _) = load_app_config_with_warnings(false);
    let mut disabled = resolve_disabled_plugins(&doc);
    if enable {
        disabled.retain(|entry| entry != plugin_id);
    } else if !disabled.iter().any(|entry| entry == plugin_id) {
        disabled.push(plugin_id.to_string());
    }
    persist_disabled_plugins(config_path.as_path(), &disabled)
}
