use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    Application,
    Service,
    Module,
    Unclassified,
    Test,
}

impl Default for PluginType {
    fn default() -> Self {
        Self::Unclassified
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntimeKind {
    Native,
    Python,
    Lua,
    External,
}

impl Default for PluginRuntimeKind {
    fn default() -> Self {
        Self::Native
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub plugin_type: PluginType,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginRuntimeSpec {
    #[serde(default)]
    pub kind: PluginRuntimeKind,
    #[serde(default)]
    pub entrypoint: String,
    #[serde(default)]
    pub module: String,
    #[serde(default)]
    pub abi: String,
    #[serde(default)]
    pub min_version: String,
    #[serde(default)]
    pub options: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginSdkSpec {
    #[serde(default = "default_sdk_version")]
    pub api_version: String,
    #[serde(default)]
    pub min_host_version: String,
    #[serde(default)]
    pub options: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub metadata: PluginMetadata,
    #[serde(default)]
    pub runtime: PluginRuntimeSpec,
    #[serde(default)]
    pub sdk: PluginSdkSpec,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(skip)]
    pub manifest_path: Option<PathBuf>,
}

impl PluginDescriptor {
    pub fn from_metadata(metadata: PluginMetadata) -> Self {
        Self {
            metadata,
            ..Self::default()
        }
    }
}

fn default_sdk_version() -> String {
    "0.1".to_string()
}
