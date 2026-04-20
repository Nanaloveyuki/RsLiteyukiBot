use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::model::{normalize_plugin_permission, supported_plugin_permissions};
use super::{
    PluginCommandDescriptor, PluginDescriptor, PluginMetadata, PluginRuntimeSpec, PluginSdkSpec,
    PluginType,
};

#[derive(Debug, Clone)]
pub enum PluginManifestError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for PluginManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "plugin manifest IO error: {}", message),
            Self::Parse(message) => write!(f, "plugin manifest parse error: {}", message),
        }
    }
}

impl std::error::Error for PluginManifestError {}

#[derive(Debug, Clone)]
pub struct PluginManifest {
    pub descriptor: PluginDescriptor,
    pub path: PathBuf,
}

pub struct PluginManifestLoader;

impl PluginManifestLoader {
    pub fn discover_in_dirs<I, P>(dirs: I) -> Result<Vec<PluginManifest>, PluginManifestError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut manifests = Vec::new();
        for dir in dirs {
            let dir = dir.as_ref();
            if !dir.exists() {
                continue;
            }

            if dir.is_file() {
                if dir.file_name().and_then(|name| name.to_str()) == Some("plugin.json") {
                    manifests.push(Self::load_manifest(dir)?);
                }
                continue;
            }

            let root_manifest = dir.join("plugin.json");
            if root_manifest.is_file() {
                manifests.push(Self::load_manifest(&root_manifest)?);
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
                    manifests.push(Self::load_manifest(&manifest)?);
                }
            }
        }
        Ok(manifests)
    }

    pub fn load_manifest(path: &Path) -> Result<PluginManifest, PluginManifestError> {
        let content = std::fs::read_to_string(path).map_err(|err| {
            PluginManifestError::Io(format!(
                "read manifest failed for {}: {}",
                path.display(),
                err
            ))
        })?;
        let raw: RawManifest = serde_json::from_str(&content).map_err(|err| {
            PluginManifestError::Parse(format!(
                "parse manifest failed for {}: {}",
                path.display(),
                err
            ))
        })?;
        let id = raw.id.unwrap_or_else(|| normalize_plugin_id(&raw.name));
        let mut descriptor = PluginDescriptor {
            metadata: PluginMetadata {
                id,
                name: raw.name,
                description: raw.description.unwrap_or_default(),
                plugin_type: raw.plugin_type.unwrap_or_default(),
                author: raw.author.unwrap_or_default(),
                homepage: raw.homepage.unwrap_or_default(),
                extra: raw.extra.unwrap_or_default(),
            },
            runtime: raw.runtime.unwrap_or_default(),
            sdk: raw.sdk.unwrap_or_default(),
            permissions: raw.permissions.unwrap_or_default(),
            commands: raw.commands.unwrap_or_default(),
            manifest_path: Some(path.to_path_buf()),
        };
        validate_manifest_commands(descriptor.commands.as_mut_slice(), path)?;
        descriptor.permissions =
            normalize_manifest_permissions(descriptor.permissions.as_slice(), path)?;
        Ok(PluginManifest {
            descriptor,
            path: path.to_path_buf(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    id: Option<String>,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "type", alias = "plugin_type")]
    plugin_type: Option<PluginType>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    extra: Option<HashMap<String, Value>>,
    #[serde(default)]
    runtime: Option<PluginRuntimeSpec>,
    #[serde(default)]
    sdk: Option<PluginSdkSpec>,
    #[serde(default)]
    permissions: Option<Vec<String>>,
    #[serde(default)]
    commands: Option<Vec<PluginCommandDescriptor>>,
}

fn normalize_plugin_id(name: &str) -> String {
    let mut id = String::with_capacity(name.len());
    let mut last_dash = false;
    for ch in name.chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            id.push(normalized);
            last_dash = false;
        } else if !last_dash {
            id.push('-');
            last_dash = true;
        }
    }
    id.trim_matches('-').to_string()
}

fn validate_manifest_commands(
    commands: &mut [PluginCommandDescriptor],
    path: &Path,
) -> Result<(), PluginManifestError> {
    let mut seen = HashSet::new();
    for (command_idx, command) in commands.iter_mut().enumerate() {
        command.name = normalize_manifest_command_name(command.name.as_str()).ok_or_else(|| {
            PluginManifestError::Parse(format!(
                "invalid manifest command in {}: commands[{command_idx}].name should not be empty",
                path.display()
            ))
        })?;
        command.description = command.description.trim().to_string();
        command.scopes =
            normalize_manifest_command_scopes(command.scopes.as_slice(), command_idx, path)?;
        if !seen.insert(command.name.clone()) {
            return Err(PluginManifestError::Parse(format!(
                "invalid manifest command in {}: commands[{command_idx}].name '{}' is duplicated",
                path.display(),
                command.name
            )));
        }
    }
    Ok(())
}

fn normalize_manifest_command_name(raw: &str) -> Option<String> {
    let name = raw.split_whitespace().next()?.trim();
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        Some(name.to_ascii_lowercase())
    } else {
        Some(format!("/{}", name.to_ascii_lowercase()))
    }
}

fn normalize_manifest_command_scopes(
    raw_scopes: &[String],
    command_idx: usize,
    path: &Path,
) -> Result<Vec<String>, PluginManifestError> {
    if raw_scopes.is_empty() {
        return Ok(vec!["all".to_string()]);
    }

    let mut scopes = Vec::new();
    for (scope_idx, raw_scope) in raw_scopes.iter().enumerate() {
        let scope = normalize_manifest_command_scope(raw_scope.as_str()).map_err(|message| {
            PluginManifestError::Parse(format!(
                "invalid manifest command in {}: commands[{command_idx}].scopes[{scope_idx}] {}",
                path.display(),
                message
            ))
        })?;
        if !scopes.iter().any(|existing| existing == &scope) {
            scopes.push(scope);
        }
    }
    Ok(scopes)
}

fn normalize_manifest_command_scope(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("should not be empty".to_string());
    }
    let compact = trimmed.to_ascii_lowercase().replace([' ', '_', '-'], "");
    match compact.as_str() {
        "all" => Ok("all".to_string()),
        "tui" => Ok("tui".to_string()),
        "adapter:onebot11" | "adapter:onebotv11" | "adapteronebot11" | "onebot11" | "onebotv11" => {
            Ok("adapter:onebot11".to_string())
        }
        _ => Err(format!(
            "uses unsupported scope '{}' (supported: all, tui, adapter:onebot11)",
            trimmed
        )),
    }
}

fn normalize_manifest_permissions(
    raw_permissions: &[String],
    path: &Path,
) -> Result<Vec<String>, PluginManifestError> {
    let mut permissions = Vec::new();
    for (permission_idx, raw_permission) in raw_permissions.iter().enumerate() {
        let Some(permission) = normalize_plugin_permission(raw_permission) else {
            return Err(PluginManifestError::Parse(format!(
                "invalid manifest permission in {}: permissions[{permission_idx}] uses unsupported permission '{}' (supported: {})",
                path.display(),
                raw_permission.trim(),
                supported_plugin_permissions().join(", ")
            )));
        };
        if !permissions.iter().any(|existing| existing == &permission) {
            permissions.push(permission);
        }
    }
    Ok(permissions)
}
