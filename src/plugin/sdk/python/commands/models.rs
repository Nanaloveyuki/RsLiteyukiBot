use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginTuiCommand {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginScopedCommand {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub plugin_id: String,
    pub scopes: Vec<String>,
    pub executable_in_tui: bool,
}
