use std::fs;
use std::path::Path;

use serde_json::Value;

use super::{McpConfigFile, McpServerConfig};

pub(super) fn read_server_configs(
    config_path: &Path,
) -> Result<Option<Vec<McpServerConfig>>, String> {
    if !config_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read MCP config {}: {err}", config_path.display()))?;
    let parsed = serde_json::from_str::<Value>(content.as_str()).map_err(|err| {
        format!(
            "failed to parse MCP config {}: {err}",
            config_path.display()
        )
    })?;

    if parsed.is_array() {
        let servers = serde_json::from_value::<Vec<McpServerConfig>>(parsed).map_err(|err| {
            format!(
                "failed to decode MCP config array {}: {err}",
                config_path.display()
            )
        })?;
        return Ok(Some(servers));
    }

    let config = serde_json::from_value::<McpConfigFile>(parsed).map_err(|err| {
        format!(
            "failed to decode MCP config object {}: {err}",
            config_path.display()
        )
    })?;
    Ok(Some(config.servers))
}

pub(super) fn normalized_transport(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("streamable_http")
        .to_ascii_lowercase()
}
