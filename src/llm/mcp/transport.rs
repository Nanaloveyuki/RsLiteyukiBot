use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::llm::client::{LlmClientError, LlmToolOutput};

use super::McpServerConfig;
use super::rpc::{
    McpHttpResponse, extract_json_rpc_result, parse_call_tool_output, parse_sse_json_payload,
};

pub(super) const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";
const DEFAULT_MCP_TIMEOUT_SECONDS: u64 = 20;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct McpRemoteTool {
    pub(super) name: String,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default, alias = "input_schema")]
    #[serde(rename = "inputSchema")]
    pub(super) input_schema: Option<Value>,
}

#[derive(Clone)]
pub(super) struct StreamableHttpMcpClient {
    server: Arc<McpServerConfig>,
    client: Client,
}

impl StreamableHttpMcpClient {
    pub(super) fn new(server: McpServerConfig) -> Result<Self, String> {
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

    pub(super) async fn list_tools(&self) -> Result<Vec<McpRemoteTool>, String> {
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

    pub(super) async fn call_tool(
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
