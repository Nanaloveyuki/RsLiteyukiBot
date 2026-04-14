use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::{PluginDescriptor, PluginMetadata, PluginRuntimeSpec, PluginSdkSpec, PluginType};

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
            PluginManifestError::Io(format!("read manifest failed for {}: {}", path.display(), err))
        })?;
        let raw: RawManifest = serde_json::from_str(&content).map_err(|err| {
            PluginManifestError::Parse(format!(
                "parse manifest failed for {}: {}",
                path.display(),
                err
            ))
        })?;
        let id = raw.id.unwrap_or_else(|| normalize_plugin_id(&raw.name));
        let descriptor = PluginDescriptor {
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
            manifest_path: Some(path.to_path_buf()),
        };
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

