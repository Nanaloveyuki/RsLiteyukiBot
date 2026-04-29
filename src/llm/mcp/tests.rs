use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::rpc::{parse_call_tool_output, parse_sse_json_payload};
use super::schema::normalize_mcp_input_schema;
use super::transport::DEFAULT_PROTOCOL_VERSION;
use super::*;
use crate::llm::client::LlmToolOutput;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};

#[test]
// 必要测试
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
// 必要测试
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
// 必要测试
fn parse_sse_json_payload_prefers_result_event_over_notification() {
    let payload = parse_sse_json_payload(
        "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progress\":0.5}}\n\n\
         data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n",
    )
    .expect("payload should parse");

    assert_eq!(payload["result"]["tools"], json!([]));
}

#[tokio::test]
// 必要测试
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
// 必要测试
async fn mcp_manager_loads_tools_from_config_file() {
    let _lock = process_state_lock();
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
// 必要测试
async fn mcp_manager_surfaces_invalid_config_as_warning() {
    let _lock = process_state_lock();
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
// 必要测试
async fn inspect_servers_returns_tool_names_for_active_server() {
    let _lock = process_state_lock();
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
