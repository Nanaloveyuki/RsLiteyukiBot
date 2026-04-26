use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config_paths::resolve_preferred_mcp_config_path;
use crate::llm::client::{LlmClientError, LlmFunctionTool, LlmToolOutput};
use liteyukibot_core::SseParser;

const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";
const DEFAULT_MCP_TIMEOUT_SECONDS: u64 = 20;

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

#[derive(Debug, Clone, Deserialize)]
struct McpRemoteTool {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "input_schema")]
    #[serde(rename = "inputSchema")]
    input_schema: Option<Value>,
}

#[derive(Clone)]
struct StreamableHttpMcpClient {
    server: Arc<McpServerConfig>,
    client: Client,
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
        let configs = match self.read_server_configs() {
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
        let configs = match self.read_server_configs() {
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

    fn read_server_configs(&self) -> Result<Option<Vec<McpServerConfig>>, String> {
        if !self.config_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.config_path).map_err(|err| {
            format!(
                "failed to read MCP config {}: {err}",
                self.config_path.display()
            )
        })?;
        let parsed = serde_json::from_str::<Value>(content.as_str()).map_err(|err| {
            format!(
                "failed to parse MCP config {}: {err}",
                self.config_path.display()
            )
        })?;

        if parsed.is_array() {
            let servers =
                serde_json::from_value::<Vec<McpServerConfig>>(parsed).map_err(|err| {
                    format!(
                        "failed to decode MCP config array {}: {err}",
                        self.config_path.display()
                    )
                })?;
            return Ok(Some(servers));
        }

        let config = serde_json::from_value::<McpConfigFile>(parsed).map_err(|err| {
            format!(
                "failed to decode MCP config object {}: {err}",
                self.config_path.display()
            )
        })?;
        Ok(Some(config.servers))
    }
}

impl StreamableHttpMcpClient {
    fn new(server: McpServerConfig) -> Result<Self, String> {
        let timeout_seconds = server
            .timeout_seconds
            .unwrap_or(DEFAULT_MCP_TIMEOUT_SECONDS);
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds.max(1)))
            .build()
            .map_err(|err| format!("reqwest init failed: {err}"))?;

        Ok(Self {
            server: Arc::new(server),
            client,
        })
    }

    async fn list_tools(&self) -> Result<Vec<McpRemoteTool>, String> {
        let session_id = self.initialize_session().await?;
        let payload = self
            .send_request(session_id.as_deref(), 2, "tools/list", json!({}))
            .await?;
        let result = extract_json_rpc_result(payload.payload.as_ref())?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| "MCP tools/list result did not contain a tools array".to_string())?;

        tools
            .iter()
            .cloned()
            .map(|tool| {
                serde_json::from_value::<McpRemoteTool>(tool)
                    .map_err(|err| format!("failed to parse MCP tool entry: {err}"))
            })
            .collect()
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<LlmToolOutput, LlmClientError> {
        let session_id = self
            .initialize_session()
            .await
            .map_err(LlmClientError::Tool)?;
        let arguments = arguments.as_object().cloned().ok_or_else(|| {
            LlmClientError::Tool(format!("MCP tool '{tool_name}' requires object arguments"))
        })?;
        let payload = self
            .send_request(
                session_id.as_deref(),
                2,
                "tools/call",
                json!({
                    "name": tool_name,
                    "arguments": arguments,
                }),
            )
            .await
            .map_err(LlmClientError::Tool)?;
        parse_call_tool_output(payload.payload.as_ref()).map_err(LlmClientError::Tool)
    }

    async fn initialize_session(&self) -> Result<Option<String>, String> {
        let payload = self
            .send_request(
                None,
                0,
                "initialize",
                json!({
                    "protocolVersion": DEFAULT_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "liteyuki-mcp-client",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
            )
            .await?;
        let session_id = payload.session_id.clone();
        self.send_notification(
            session_id.as_deref(),
            "notifications/initialized",
            json!({}),
        )
        .await?;
        Ok(session_id)
    }

    async fn send_notification(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<(), String> {
        let response = self
            .build_request(session_id)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            }))
            .send()
            .await
            .map_err(|err| format!("failed to call MCP notification '{method}': {err}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            return Err(format!(
                "MCP notification '{method}' failed with status {status}: {body}"
            ));
        }
        Ok(())
    }

    async fn send_request(
        &self,
        session_id: Option<&str>,
        id: u64,
        method: &str,
        params: Value,
    ) -> Result<McpHttpResponse, String> {
        let response = self
            .build_request(session_id)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .send()
            .await
            .map_err(|err| format!("failed to call MCP method '{method}': {err}"))?;

        let status = response.status();
        let headers = response.headers().clone();
        let session_id = headers
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string);
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response
            .text()
            .await
            .map_err(|err| format!("failed to read MCP response body: {err}"))?;

        if !status.is_success() {
            return Err(format!(
                "MCP method '{method}' failed with status {status}: {body}"
            ));
        }

        let payload = if body.trim().is_empty() {
            None
        } else if content_type.contains("text/event-stream") {
            Some(parse_sse_json_payload(body.as_str())?)
        } else {
            Some(
                serde_json::from_str::<Value>(body.as_str())
                    .map_err(|err| format!("failed to decode MCP JSON body: {err}"))?,
            )
        };

        Ok(McpHttpResponse {
            payload,
            session_id,
        })
    }

    fn build_request(&self, session_id: Option<&str>) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .post(self.server.url.as_str())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            );
        for (name, value) in &self.server.headers {
            request = request.header(name, value);
        }
        if let Some(session_id) = session_id {
            request = request.header("mcp-session-id", session_id);
        }
        request
    }
}

struct McpHttpResponse {
    payload: Option<Value>,
    session_id: Option<String>,
}

fn parse_sse_json_payload(body: &str) -> Result<Value, String> {
    let mut parser = SseParser::default();
    let events = parser.push_chunk(body);
    let mut fallback_payload = None;
    for event in events {
        let data = event.data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(payload) = serde_json::from_str::<Value>(data) {
            if payload.get("result").is_some() || payload.get("error").is_some() {
                return Ok(payload);
            }
            fallback_payload = Some(payload);
        }
    }
    if let Some(payload) = fallback_payload {
        if payload.is_object() {
            return Ok(payload);
        }
    }
    Err("MCP SSE response did not contain a JSON payload".to_string())
}

fn extract_json_rpc_result(payload: Option<&Value>) -> Result<&Value, String> {
    let payload =
        payload.ok_or_else(|| "MCP response did not contain a JSON payload".to_string())?;
    if let Some(error) = payload.get("error") {
        return Err(format!("MCP JSON-RPC error: {error}"));
    }
    payload
        .get("result")
        .ok_or_else(|| "MCP JSON-RPC response missing result".to_string())
}

fn parse_call_tool_output(payload: Option<&Value>) -> Result<LlmToolOutput, String> {
    let result = extract_json_rpc_result(payload)?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(format!(
            "remote MCP tool returned isError=true: {}",
            render_call_result_text(result).unwrap_or_else(|| result.to_string())
        ));
    }

    if result.get("structuredContent").is_some() {
        return Ok(LlmToolOutput::Json(result.clone()));
    }
    if let Some(text) = render_call_result_text(result) {
        return Ok(LlmToolOutput::Text(text));
    }
    Ok(LlmToolOutput::Json(result.clone()))
}

fn render_call_result_text(result: &Value) -> Option<String> {
    let mut fragments = Vec::new();
    for item in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = item.get("text").and_then(Value::as_str)
        {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                fragments.push(trimmed.to_string());
            }
        }
    }

    if fragments.is_empty() {
        None
    } else {
        Some(fragments.join("\n"))
    }
}

fn normalized_transport(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("streamable_http")
        .to_ascii_lowercase()
}

fn namespace_tool_name(server_name: &str, tool_name: &str) -> String {
    format!(
        "mcp__{}__{}",
        sanitize_identifier(server_name),
        sanitize_identifier(tool_name)
    )
}

fn sanitize_identifier(raw: &str) -> String {
    let mut output = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if ch == '_' || ch == '-' {
            output.push('_');
        }
    }

    let trimmed = output.trim_matches('_');
    if trimmed.is_empty() {
        "tool".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_mcp_input_schema(schema: Value) -> Value {
    fn normalize(node: &Value) -> Value {
        match node {
            Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
            Value::Object(object) => {
                let mut normalized = serde_json::Map::new();
                for (key, value) in object {
                    normalized.insert(key.clone(), normalize(value));
                }

                let original_properties = object
                    .get("properties")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let mut required = normalized
                    .get("required")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if let Some(properties) = normalized
                    .get_mut("properties")
                    .and_then(Value::as_object_mut)
                {
                    for (name, property) in properties.iter_mut() {
                        let Some(original_property) =
                            original_properties.get(name).and_then(Value::as_object)
                        else {
                            continue;
                        };
                        let Some(required_flag) =
                            original_property.get("required").and_then(Value::as_bool)
                        else {
                            continue;
                        };
                        if let Some(property_object) = property.as_object_mut() {
                            property_object.remove("required");
                        }
                        if required_flag {
                            required.push(Value::String(name.clone()));
                        }
                    }

                    if required.is_empty() {
                        normalized.remove("required");
                    } else {
                        let mut unique = Vec::new();
                        for value in required {
                            if unique.iter().all(|existing| existing != &value) {
                                unique.push(value);
                            }
                        }
                        normalized.insert("required".to_string(), Value::Array(unique));
                    }
                }

                Value::Object(normalized)
            }
            _ => node.clone(),
        }
    }

    normalize(&schema)
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[test]
    fn normalize_schema_lifts_boolean_required_flags() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "required": true
                },
                "optional": {
                    "type": "string",
                    "required": false
                }
            }
        });

        let normalized = normalize_mcp_input_schema(schema);
        assert_eq!(normalized["required"], json!(["path"]));
        assert!(normalized["properties"]["path"].get("required").is_none());
        assert!(
            normalized["properties"]["optional"]
                .get("required")
                .is_none()
        );
    }

    #[test]
    fn parse_call_tool_output_prefers_text_when_available() {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [
                    { "type": "text", "text": "hello" }
                ]
            }
        });

        let output = parse_call_tool_output(Some(&payload)).expect("output should parse");
        assert_eq!(output, LlmToolOutput::Text("hello".to_string()));
    }

    #[test]
    fn parse_sse_json_payload_prefers_result_event_over_notification() {
        let payload = parse_sse_json_payload(
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progress\":0.5}}\n\n\
             data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n",
        )
        .expect("payload should parse");

        assert_eq!(payload["result"]["tools"], json!([]));
    }

    #[tokio::test]
    async fn streamable_http_client_calls_remote_tool() {
        let (url, server_task) = spawn_mock_mcp_server(MockMcpMode::CallTool)
            .await
            .expect("mock server should start");
        let client = StreamableHttpMcpClient::new(McpServerConfig {
            name: "mock".to_string(),
            url,
            active: true,
            transport: Some("streamable_http".to_string()),
            headers: HashMap::new(),
            timeout_seconds: Some(5),
        })
        .expect("client should build");

        let output = client
            .call_tool("lookup_weather", json!({"city": "Paris"}))
            .await
            .expect("tool call should succeed");
        assert_eq!(output, LlmToolOutput::Text("Sunny in Paris".to_string()));

        server_task.await.expect("mock server should finish");
    }

    #[tokio::test]
    async fn mcp_manager_loads_tools_from_config_file() {
        let _lock = TEST_ENV_LOCK
            .lock()
            .expect("env lock should not be poisoned");
        let (url, server_task) = spawn_mock_mcp_server(MockMcpMode::ListTools)
            .await
            .expect("mock server should start");
        let config_path = temp_path("mcp-config").with_extension("json");
        fs::write(
            &config_path,
            json!({
                "servers": [
                    {
                        "name": "mock",
                        "url": url,
                        "active": true,
                        "transport": "streamable_http"
                    }
                ]
            })
            .to_string(),
        )
        .expect("config should be written");
        let _guard = EnvVarGuard::set("LY_MCP_CONFIG_PATH", config_path.as_path());

        let manager = McpManager::from_default_config();
        let loaded = manager.load_tools().await;
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.tools.len(), 1);
        assert_eq!(loaded.tools[0].name, "mcp__mock__lookup_weather");
        assert_eq!(loaded.tools[0].remote_name, "lookup_weather");
        assert_eq!(loaded.tools[0].parameters["required"], json!(["city"]));

        server_task.await.expect("mock server should finish");
        let _ = fs::remove_file(config_path);
    }

    #[tokio::test]
    async fn mcp_manager_surfaces_invalid_config_as_warning() {
        let _lock = TEST_ENV_LOCK
            .lock()
            .expect("env lock should not be poisoned");
        let config_path = temp_path("mcp-config-invalid").with_extension("json");
        fs::write(&config_path, "{ invalid json").expect("config should be written");
        let _guard = EnvVarGuard::set("LY_MCP_CONFIG_PATH", config_path.as_path());

        let manager = McpManager::from_default_config();
        let loaded = manager.load_tools().await;
        assert!(loaded.tools.is_empty());
        assert_eq!(loaded.warnings.len(), 1);
        assert!(loaded.warnings[0].contains("failed to parse MCP config"));

        let _ = fs::remove_file(config_path);
    }

    #[tokio::test]
    async fn inspect_servers_returns_tool_names_for_active_server() {
        let _lock = TEST_ENV_LOCK
            .lock()
            .expect("env lock should not be poisoned");
        let (url, server_task) = spawn_mock_mcp_server(MockMcpMode::ListTools)
            .await
            .expect("mock server should start");
        let config_path = temp_path("mcp-inspect").with_extension("json");
        fs::write(
            &config_path,
            json!([
                {
                    "name": "mock",
                    "url": url,
                    "active": true,
                    "transport": "streamable_http"
                }
            ])
            .to_string(),
        )
        .expect("config should be written");
        let _guard = EnvVarGuard::set("LY_MCP_CONFIG_PATH", config_path.as_path());

        let snapshot = McpManager::from_default_config().inspect_servers().await;
        assert!(snapshot.warnings.is_empty());
        assert_eq!(snapshot.servers.len(), 1);
        assert_eq!(snapshot.servers[0].tool_count, 1);
        assert_eq!(snapshot.servers[0].tool_names, vec!["lookup_weather"]);

        server_task.await.expect("mock server should finish");
        let _ = fs::remove_file(config_path);
    }

    #[derive(Clone, Copy)]
    enum MockMcpMode {
        ListTools,
        CallTool,
    }

    static TEST_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests control these process-wide environment updates and restore them on drop.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => {
                    // SAFETY: tests restore the original process environment value captured before mutation.
                    unsafe {
                        std::env::set_var(self.key, value);
                    }
                }
                None => {
                    // SAFETY: tests restore the environment to its original unset state.
                    unsafe {
                        std::env::remove_var(self.key);
                    }
                }
            }
        }
    }

    async fn spawn_mock_mcp_server(
        mode: MockMcpMode,
    ) -> Result<(String, tokio::task::JoinHandle<()>), String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|err| format!("bind failed: {err}"))?;
        let address = listener
            .local_addr()
            .map_err(|err| format!("addr failed: {err}"))?;
        let handle = tokio::spawn(async move {
            let expected_requests = match mode {
                MockMcpMode::ListTools => 3,
                MockMcpMode::CallTool => 3,
            };

            for _ in 0..expected_requests {
                let (mut socket, _) = listener.accept().await.expect("accept should succeed");
                let request = read_http_request_json(&mut socket)
                    .await
                    .expect("request should parse");
                let method = request
                    .get("method")
                    .and_then(Value::as_str)
                    .expect("method should exist");
                match method {
                    "initialize" => {
                        write_json_response(
                            &mut socket,
                            200,
                            Some("test-session"),
                            &json!({
                                "jsonrpc": "2.0",
                                "id": request["id"],
                                "result": {
                                    "protocolVersion": DEFAULT_PROTOCOL_VERSION,
                                    "capabilities": {
                                        "tools": {}
                                    },
                                    "serverInfo": {
                                        "name": "mock",
                                        "version": "1.0.0"
                                    }
                                }
                            }),
                        )
                        .await
                        .expect("initialize response should write");
                    }
                    "notifications/initialized" => {
                        write_empty_response(&mut socket, 202)
                            .await
                            .expect("notification response should write");
                    }
                    "tools/list" => {
                        write_json_response(
                            &mut socket,
                            200,
                            None,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": request["id"],
                                "result": {
                                    "tools": [
                                        {
                                            "name": "lookup_weather",
                                            "description": "Lookup weather by city",
                                            "inputSchema": {
                                                "type": "object",
                                                "properties": {
                                                    "city": {
                                                        "type": "string",
                                                        "required": true
                                                    }
                                                }
                                            }
                                        }
                                    ]
                                }
                            }),
                        )
                        .await
                        .expect("tools/list response should write");
                    }
                    "tools/call" => {
                        assert_eq!(request["params"]["name"], "lookup_weather");
                        assert_eq!(request["params"]["arguments"]["city"], "Paris");
                        write_json_response(
                            &mut socket,
                            200,
                            None,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": request["id"],
                                "result": {
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": "Sunny in Paris"
                                        }
                                    ]
                                }
                            }),
                        )
                        .await
                        .expect("tools/call response should write");
                    }
                    other => panic!("unexpected MCP method: {other}"),
                }
            }
        });

        Ok((format!("http://{address}/mcp"), handle))
    }

    async fn read_http_request_json(socket: &mut TcpStream) -> Result<Value, String> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = socket
                .read(&mut chunk)
                .await
                .map_err(|err| format!("read failed: {err}"))?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(headers_end) = find_double_crlf(&buffer) {
                let headers = String::from_utf8_lossy(&buffer[..headers_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                let body_start = headers_end + 4;
                if buffer.len() >= body_start + content_length {
                    let body = &buffer[body_start..body_start + content_length];
                    return serde_json::from_slice(body)
                        .map_err(|err| format!("body decode failed: {err}"));
                }
            }
        }
        Err("request closed before full body was received".to_string())
    }

    async fn write_json_response(
        socket: &mut TcpStream,
        status: u16,
        session_id: Option<&str>,
        payload: &Value,
    ) -> Result<(), String> {
        let body = payload.to_string();
        let status_text = match status {
            200 => "OK",
            202 => "Accepted",
            _ => "OK",
        };
        let mut headers = format!(
            "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        if let Some(session_id) = session_id {
            headers.push_str(format!("mcp-session-id: {session_id}\r\n").as_str());
        }
        headers.push_str("\r\n");
        socket
            .write_all(format!("{headers}{body}").as_bytes())
            .await
            .map_err(|err| format!("write failed: {err}"))
    }

    async fn write_empty_response(socket: &mut TcpStream, status: u16) -> Result<(), String> {
        let status_text = match status {
            202 => "Accepted",
            _ => "OK",
        };
        socket
            .write_all(
                format!(
                    "HTTP/1.1 {status} {status_text}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .map_err(|err| format!("write failed: {err}"))
    }

    fn find_double_crlf(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn temp_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("liteyuki-mcp-test-{label}-{unique}"))
    }
}
