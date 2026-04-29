#[path = "synthesis/merge.rs"]
mod merge;
#[path = "synthesis/source_extra.rs"]
mod source_extra;

use std::path::Path;

use serde_json::Value;

use crate::plugin::loader::{
    PluginManifest, PluginManifestError, normalize_manifest_permissions, normalize_plugin_id,
    validate_manifest_commands,
};
use crate::plugin::{PluginDescriptor, PluginMetadata, PluginType};

use super::super::family::build_family_seed;
use super::super::model::*;
use super::paths::{default_plugin_config_path, resolve_source_root};

pub(super) fn load_override_manifest(
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
    let mut runtime = merge::merge_runtime(seed.runtime, host.runtime.unwrap_or_default());
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
    let sdk = merge::merge_sdk(seed.sdk, host.sdk.unwrap_or_default());
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
    source_extra::inject_source_extra(
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
