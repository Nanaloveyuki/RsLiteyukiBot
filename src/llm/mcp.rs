use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::llm::client::LlmFunctionTool;
#[path = "mcp/config.rs"]
mod config;
#[path = "mcp/rpc.rs"]
mod rpc;
#[path = "mcp/schema.rs"]
mod schema;
#[path = "mcp/transport.rs"]
mod transport;

use self::config::{normalized_transport, read_server_configs};
use self::schema::{namespace_tool_name, normalize_mcp_input_schema};
use self::transport::StreamableHttpMcpClient;
use crate::utils::config_path::resolve_preferred_mcp_config_path;

#[derive(Debug, Clone, Default)]
pub(crate) struct McpLoadResult {
    pub(crate) tools: Vec<McpBoundTool>,
    pub(crate) warnings: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpServerCatalogSnapshot {
    pub(crate) servers: Vec<McpServerCatalogEntry>,
    pub(crate) warnings: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpServerCatalogEntry {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) transport: String,
    pub(crate) active: bool,
    pub(crate) tool_count: usize,
    pub(crate) tool_names: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpBoundTool {
    pub(crate) server_name: String,
    pub(crate) name: String,
    pub(crate) remote_name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
    pub(crate) tool: LlmFunctionTool,
}

#[derive(Debug, Clone)]
pub(crate) struct McpManager {
    config_path: std::path::PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpServerConfig {
    pub(crate) name: String,
    pub(crate) url: String,
    #[serde(default = "default_true")]
    pub(crate) active: bool,
    #[serde(default, alias = "type")]
    pub(crate) transport: Option<String>,
    #[serde(default)]
    pub(crate) headers: HashMap<String, String>,
    #[serde(default, alias = "timeout_seconds")]
    pub(crate) timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct McpConfigFile {
    #[serde(default)]
    servers: Vec<McpServerConfig>,
}

fn default_true() -> bool {
    true
}

impl McpManager {
    pub(crate) fn from_default_config() -> Self {
        Self::from_config_path(resolve_preferred_mcp_config_path())
    }

    pub(crate) fn from_config_path(config_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            config_path: config_path.into(),
        }
    }

    pub(crate) async fn load_tools(&self) -> McpLoadResult {
        let configs = match read_server_configs(self.config_path.as_path()) {
            Ok(Some(configs)) => configs,
            Ok(None) => return McpLoadResult::default(),
            Err(err) => {
                return McpLoadResult {
                    tools: Vec::new(),
                    warnings: vec![err],
                };
            }
        };

        let mut loaded = Vec::new();
        let mut seen_tool_names = std::collections::HashSet::new();
        let mut warnings = Vec::new();
        for config in configs {
            if !config.active {
                continue;
            }

            let transport = normalized_transport(config.transport.as_deref());
            if transport != "streamable_http" && transport != "http" {
                warnings.push(format!(
                    "skipped MCP server '{}' because transport '{}' is not supported yet",
                    config.name, transport
                ));
                continue;
            }

            let client = match StreamableHttpMcpClient::new(config.clone()) {
                Ok(client) => client,
                Err(err) => {
                    warnings.push(format!(
                        "failed to prepare MCP server '{}': {err}",
                        config.name
                    ));
                    continue;
                }
            };

            let tools = match client.list_tools().await {
                Ok(tools) => tools,
                Err(err) => {
                    warnings.push(format!(
                        "failed to list tools from MCP server '{}': {err}",
                        config.name
                    ));
                    continue;
                }
            };

            for remote_tool in tools {
                let remote_tool_name = remote_tool.name.clone();
                let tool_name =
                    namespace_tool_name(config.name.as_str(), remote_tool_name.as_str());
                if !seen_tool_names.insert(tool_name.clone()) {
                    warnings.push(format!(
                        "skipped duplicate MCP tool name '{}' from server '{}'",
                        tool_name, config.name
                    ));
                    continue;
                }
                let description = remote_tool.description.unwrap_or_default();
                let parameters = normalize_mcp_input_schema(
                    remote_tool
                        .input_schema
                        .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                );
                let handler_client = client.clone();
                let handler_tool_name = remote_tool_name.clone();
                let tool =
                    LlmFunctionTool::new(tool_name.clone(), parameters.clone(), move |arguments| {
                        let client = handler_client.clone();
                        let tool_name = handler_tool_name.clone();
                        async move { client.call_tool(tool_name.as_str(), arguments).await }
                    })
                    .with_description(description.clone());

                loaded.push(McpBoundTool {
                    server_name: config.name.clone(),
                    name: tool_name,
                    remote_name: remote_tool_name,
                    description,
                    parameters,
                    tool,
                });
            }
        }

        McpLoadResult {
            tools: loaded,
            warnings,
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn inspect_servers(&self) -> McpServerCatalogSnapshot {
        let configs = match read_server_configs(self.config_path.as_path()) {
            Ok(Some(configs)) => configs,
            Ok(None) => return McpServerCatalogSnapshot::default(),
            Err(err) => {
                return McpServerCatalogSnapshot {
                    servers: Vec::new(),
                    warnings: vec![err],
                };
            }
        };

        let mut servers = Vec::with_capacity(configs.len());
        for config in configs {
            let transport = normalized_transport(config.transport.as_deref());
            let mut entry = McpServerCatalogEntry {
                name: config.name.clone(),
                url: config.url.clone(),
                transport: transport.clone(),
                active: config.active,
                tool_count: 0,
                tool_names: Vec::new(),
                warnings: Vec::new(),
            };

            if !config.active {
                servers.push(entry);
                continue;
            }
            if transport != "streamable_http" && transport != "http" {
                entry
                    .warnings
                    .push(format!("transport '{}' is not supported yet", transport));
                servers.push(entry);
                continue;
            }

            let client = match StreamableHttpMcpClient::new(config.clone()) {
                Ok(client) => client,
                Err(err) => {
                    entry
                        .warnings
                        .push(format!("failed to prepare MCP client: {err}"));
                    servers.push(entry);
                    continue;
                }
            };

            match client.list_tools().await {
                Ok(tools) => {
                    entry.tool_count = tools.len();
                    entry.tool_names = tools.into_iter().map(|tool| tool.name).collect();
                }
                Err(err) => entry.warnings.push(err),
            }

            servers.push(entry);
        }

        McpServerCatalogSnapshot {
            servers,
            warnings: Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "mcp/tests.rs"]
mod tests;
