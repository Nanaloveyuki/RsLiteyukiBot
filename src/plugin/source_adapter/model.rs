use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plugin::{
    PluginCommandDescriptor, PluginRuntimeKind, PluginRuntimeSpec, PluginSdkSpec, PluginType,
};

pub(crate) const OVERRIDE_MANIFEST_DIR: &str = "manifests";
pub(crate) const OVERRIDE_MANIFEST_SUFFIX: &str = ".override.json";
pub(crate) const SYNTHETIC_MANIFEST_FILENAME: &str = ".liteyuki-source-adapter.json";

pub(crate) const EXTRA_SOURCE_FAMILY: &str = "sourceFamily";
pub(crate) const EXTRA_ADAPTER_FAMILY: &str = "adapterFamily";
pub(crate) const EXTRA_COMPAT_LEVEL: &str = "compatLevel";
pub(crate) const EXTRA_SOURCE_PATH: &str = "sourcePath";
pub(crate) const EXTRA_SOURCE_MANIFEST: &str = "sourceManifest";
pub(crate) const EXTRA_SOURCE_MANIFEST_PATH: &str = "sourceManifestPath";
pub(crate) const EXTRA_OVERRIDE_MANIFEST_PATH: &str = "overrideManifestPath";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourcePluginFamily {
    Native,
    LiteyukiPy,
    Astrbot,
    Nonebot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceAdapterFamily {
    Native,
    LiteyukiPythonBridge,
    AstrbotPythonBridge,
    NonebotExternal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceCompatLevel {
    Native,
    Bridged,
    Sidecar,
    MetadataOnly,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceOverrideManifestDoc {
    #[serde(default = "default_override_version")]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) plugin_id: Option<String>,
    pub(crate) source: SourceDescriptorDoc,
    #[serde(default)]
    pub(crate) host: Option<HostOverrideDoc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceDescriptorDoc {
    pub(crate) kind: SourcePluginFamily,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) metadata_files: Vec<String>,
    #[serde(default)]
    pub(crate) config_strategy: Option<ConfigStrategyDoc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigStrategyDoc {
    #[serde(default)]
    pub(crate) kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostOverrideDoc {
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default, rename = "type", alias = "pluginType")]
    pub(crate) plugin_type: Option<PluginType>,
    #[serde(default)]
    pub(crate) author: Option<String>,
    #[serde(default)]
    pub(crate) homepage: Option<String>,
    #[serde(default)]
    pub(crate) runtime: Option<RuntimeOverrideDoc>,
    #[serde(default)]
    pub(crate) sdk: Option<SdkOverrideDoc>,
    #[serde(default)]
    pub(crate) permissions: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) commands: Option<Vec<PluginCommandDescriptor>>,
    #[serde(default)]
    pub(crate) extra: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeOverrideDoc {
    #[serde(default)]
    pub(crate) kind: Option<PluginRuntimeKind>,
    #[serde(default)]
    pub(crate) entrypoint: Option<String>,
    #[serde(default)]
    pub(crate) module: Option<String>,
    #[serde(default)]
    pub(crate) abi: Option<String>,
    #[serde(default)]
    pub(crate) min_version: Option<String>,
    #[serde(default)]
    pub(crate) options: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SdkOverrideDoc {
    #[serde(default)]
    pub(crate) api_version: Option<String>,
    #[serde(default)]
    pub(crate) min_host_version: Option<String>,
    #[serde(default)]
    pub(crate) options: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FamilyDescriptorSeed {
    pub(crate) plugin_id_hint: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) plugin_type: Option<PluginType>,
    pub(crate) author: Option<String>,
    pub(crate) homepage: Option<String>,
    pub(crate) runtime: PluginRuntimeSpec,
    pub(crate) sdk: PluginSdkSpec,
    pub(crate) permissions: Vec<String>,
    pub(crate) commands: Vec<PluginCommandDescriptor>,
    pub(crate) extra: HashMap<String, Value>,
    pub(crate) source_manifest_name: Option<String>,
}

fn default_override_version() -> u32 {
    1
}
