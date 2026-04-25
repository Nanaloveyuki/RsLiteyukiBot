use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    Application,
    Service,
    Module,
    #[default]
    Unclassified,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntimeKind {
    #[default]
    Native,
    Python,
    Lua,
    External,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginCommandDescriptor {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub scopes: Vec<String>,
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
    #[serde(default)]
    pub commands: Vec<PluginCommandDescriptor>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapabilitySource {
    AstrbotDecorator,
    AstrbotContext,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegisteredTool {
    #[serde(default)]
    pub plugin_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub source: PluginCapabilitySource,
    #[serde(default)]
    pub handler_module_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegisteredWebApi {
    #[serde(default)]
    pub plugin_id: String,
    pub route: String,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub source: PluginCapabilitySource,
    #[serde(default)]
    pub runtime_kind: PluginRuntimeKind,
    #[serde(default)]
    pub handler_module_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegisteredCronJob {
    #[serde(default)]
    pub plugin_id: String,
    pub job_id: String,
    #[serde(default)]
    pub job_type: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub cron_expression: Option<String>,
    #[serde(default)]
    pub run_once: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub persistent: bool,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub next_run_time: Option<String>,
    #[serde(default)]
    pub last_run_time: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegisteredTask {
    #[serde(default)]
    pub plugin_id: String,
    pub task_id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub task_kind: String,
    #[serde(default)]
    pub source: PluginCapabilitySource,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapabilitySnapshot {
    pub plugin_id: String,
    #[serde(default)]
    pub runtime_kind: PluginRuntimeKind,
    #[serde(default)]
    pub tools: Vec<PluginRegisteredTool>,
    #[serde(default)]
    pub web_apis: Vec<PluginRegisteredWebApi>,
    #[serde(default)]
    pub cron_jobs: Vec<PluginRegisteredCronJob>,
    #[serde(default)]
    pub tasks: Vec<PluginRegisteredTask>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PluginToolResult {
    Text(String),
    Json(Value),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginExecutionRecord {
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_at: Option<String>,
    #[serde(default)]
    pub last_success_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntimeDiagnostics {
    #[serde(default)]
    pub plugin_id: String,
    #[serde(default)]
    pub last_web_api_dispatch: PluginExecutionRecord,
    #[serde(default)]
    pub last_tool_execution: PluginExecutionRecord,
    #[serde(default)]
    pub last_cron_execution: PluginExecutionRecord,
}

fn default_sdk_version() -> String {
    "0.1".to_string()
}

pub(crate) const PLUGIN_PERMISSION_ALLOW_ALL: &str = "*";

const SUPPORTED_PLUGIN_PERMISSIONS: [&str; 7] = [
    PLUGIN_PERMISSION_ALLOW_ALL,
    "kv.read",
    "kv.write",
    "channel.publish",
    "adapter.reply",
    "config.read",
    "config.write",
];

const SUPPORTED_PLUGIN_COMMAND_PERMISSIONS: [&str; 2] = ["command.tui.read", "command.tui.manage"];

pub(crate) fn supported_plugin_permissions() -> Vec<&'static str> {
    let mut permissions = Vec::with_capacity(
        SUPPORTED_PLUGIN_PERMISSIONS.len() + SUPPORTED_PLUGIN_COMMAND_PERMISSIONS.len(),
    );
    permissions.extend(SUPPORTED_PLUGIN_PERMISSIONS);
    permissions.extend(SUPPORTED_PLUGIN_COMMAND_PERMISSIONS);
    permissions
}

pub(crate) fn normalize_plugin_permission(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let compact = trimmed.to_ascii_lowercase().replace([' ', '_', '-'], "");
    match compact.as_str() {
        "*" | "all" => Some(PLUGIN_PERMISSION_ALLOW_ALL.to_string()),
        "kv.read" | "kvread" => Some("kv.read".to_string()),
        "kv.write" | "kvwrite" => Some("kv.write".to_string()),
        "channel.publish" | "channelpublish" | "publish" => Some("channel.publish".to_string()),
        "adapter.reply" | "adapterreply" | "reply" => Some("adapter.reply".to_string()),
        "config.read" | "configread" => Some("config.read".to_string()),
        "config.write" | "configwrite" | "config.delete" | "configdelete" => {
            Some("config.write".to_string())
        }
        "command.tui.read" | "commandtuiread" => Some("command.tui.read".to_string()),
        "command.tui.manage" | "commandtuimanage" | "command.tui.write" | "commandtuiwrite" => {
            Some("command.tui.manage".to_string())
        }
        _ => None,
    }
}
