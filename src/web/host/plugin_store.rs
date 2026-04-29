use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(super) struct PluginStoreItemDoc {
    pub(super) id: String,
    name: String,
    version: String,
    description: String,
    author: String,
    homepage: Option<String>,
    #[serde(rename = "downloadUrl")]
    download_url: String,
    tags: Vec<String>,
    #[serde(rename = "minVersion")]
    min_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct PluginStoreListDoc {
    version: String,
    #[serde(rename = "updateTime")]
    update_time: String,
    pub(super) plugins: Vec<PluginStoreItemDoc>,
}

impl Default for PluginStoreListDoc {
    fn default() -> Self {
        Self {
            version: "local.manifest.v1".to_string(),
            update_time: Utc::now().to_rfc3339(),
            plugins: Vec::new(),
        }
    }
}

fn localized_text(raw: &str) -> String {
    let snapshot = crate::i18n::current_snapshot();
    snapshot
        .messages
        .get(raw)
        .cloned()
        .unwrap_or_else(|| raw.to_string())
}

fn plugin_store_item_from_descriptor(descriptor: &crate::PluginDescriptor) -> PluginStoreItemDoc {
    let extra = &descriptor.metadata.extra;
    let extra_string = |key: &str| {
        extra
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    };
    let repository = extra_string("repository");
    let homepage = if descriptor.metadata.homepage.trim().is_empty() {
        repository.clone()
    } else {
        Some(descriptor.metadata.homepage.clone())
    };
    let mut tags = extra
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if tags.is_empty() {
        tags.push("builtin".to_string());
        tags.push(
            match descriptor.runtime.kind {
                crate::PluginRuntimeKind::Native => "native",
                crate::PluginRuntimeKind::Python => "python",
                crate::PluginRuntimeKind::Lua => "lua",
                crate::PluginRuntimeKind::External => "external",
            }
            .to_string(),
        );
    }

    PluginStoreItemDoc {
        id: descriptor.metadata.id.clone(),
        name: localized_text(descriptor.metadata.name.as_str()),
        version: extra_string("version").unwrap_or_else(|| "builtin".to_string()),
        description: localized_text(descriptor.metadata.description.as_str()),
        author: descriptor.metadata.author.clone(),
        homepage,
        download_url: repository
            .unwrap_or_else(|| descriptor.metadata.homepage.clone())
            .trim()
            .to_string(),
        tags,
        min_version: extra_string("minVersion"),
    }
}

pub(super) fn build_local_plugin_store_catalog(
    runtime_host: Option<&EmbeddedAppHost>,
) -> PluginStoreListDoc {
    let mut plugins = runtime_host
        .map(|host| {
            run_async_for_web_host(host.plugin_catalog_snapshot())
                .entries
                .into_iter()
                .map(|entry| plugin_store_item_from_descriptor(&entry.descriptor))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            let plugin_dirs = resolve_builtin_plugin_dirs();
            discover_plugin_manifests_in_dirs(plugin_dirs.iter())
                .map(|manifests| {
                    manifests
                        .into_iter()
                        .map(|manifest| plugin_store_item_from_descriptor(&manifest.descriptor))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        });
    plugins.sort_by(|left, right| left.id.cmp(&right.id));

    PluginStoreListDoc {
        version: "local.manifest.v1".to_string(),
        update_time: Utc::now().to_rfc3339(),
        plugins,
    }
}
