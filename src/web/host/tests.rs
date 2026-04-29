use super::realtime::aggregate_log_level;
use super::*;
use crate::RuntimeTarget;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};
use std::io::{Cursor, Write};
use std::time::{SystemTime, UNIX_EPOCH};

const TEST_HTML: &str = "<!doctype html><title>Shared Host</title>";
const TEST_SVG: &str = "<svg viewBox=\"0 0 1 1\"></svg>";

fn test_assets() -> WebHostAssets {
    WebHostAssets::new(WebHostAsset::text("text/html; charset=utf-8", TEST_HTML))
        .with_asset(
            "/assets/bot.svg",
            WebHostAsset::text("image/svg+xml; charset=utf-8", TEST_SVG),
        )
        .with_asset(
            "/favicon.ico",
            WebHostAsset::binary("image/x-icon", [1_u8, 2_u8, 3_u8]),
        )
}

fn test_snapshot() -> AppHostSnapshot {
    AppHostSnapshot {
        app_name: "Liteyuki".to_string(),
        status: "running".to_string(),
        runtime_target: "tauri2".to_string(),
        adapter_count: 3,
        resource_usage: crate::app_host::AppHostResourceUsage {
            cpu: crate::app_host::AppHostCpuUsage {
                system_percent: 63.2,
                process_percent: 18.6,
            },
            memory: crate::app_host::AppHostMemoryUsage {
                total_bytes: 16 * 1024 * 1024 * 1024,
                used_bytes: 7 * 1024 * 1024 * 1024,
                process_bytes: 512 * 1024 * 1024,
                system_percent: 43.75,
                process_percent: 3.125,
            },
        },
        ..AppHostSnapshot::default()
    }
}

fn test_server_with_snapshot(snapshot: AppHostSnapshot) -> WebHostService {
    WebHostService {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
        browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        dev_frontend: None,
        snapshot_provider: Arc::new(move || snapshot.clone()),
        runtime_host: None,
        assets: Arc::new(test_assets()),
        terminal_state: Arc::new(WebTerminalState::default()),
        auth: WebUiAuthManager::in_memory_for_tests(),
    }
}

fn test_server() -> WebHostService {
    test_server_with_snapshot(test_snapshot())
}

fn env_lock_guard() -> std::sync::MutexGuard<'static, ()> {
    process_state_lock()
}

struct LlmManagerRouteTestEnv {
    root: PathBuf,
    guards: Vec<EnvVarGuard>,
}

impl LlmManagerRouteTestEnv {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rsliteyuki-llm-manager-route-{nanos}"));
        fs::create_dir_all(root.as_path()).expect("temp route test dir should be created");
        let config_path = root.join("config.yaml");
        fs::write(config_path.as_path(), "core:\n  adapters: []\n")
            .expect("test config should be written");

        let guards = vec![
            EnvVarGuard::set("LY_CONFIG_PATH", config_path.as_path()),
            EnvVarGuard::set("LY_LLM_CONFIG_PATH", root.join("llm-config.yaml").as_path()),
            EnvVarGuard::set(
                "LY_LLM_PROMPT_STORE_PATH",
                root.join("llm-prompts.json").as_path(),
            ),
        ];

        Self { root, guards }
    }

    fn llm_config_path(&self) -> PathBuf {
        self.root.join("llm-config.yaml")
    }
}

impl Drop for LlmManagerRouteTestEnv {
    fn drop(&mut self) {
        self.guards.clear();
        let _ = fs::remove_dir_all(self.root.as_path());
    }
}

struct CapabilityRouteTestEnv {
    root: PathBuf,
    guards: Vec<EnvVarGuard>,
}

impl CapabilityRouteTestEnv {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rsliteyuki-capability-route-{nanos}"));
        fs::create_dir_all(root.as_path()).expect("temp capability dir should be created");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"cap-test\"\nversion = \"0.0.0\"\n",
        )
        .expect("workspace marker should be written");
        fs::create_dir_all(root.join("skills").join("demo")).expect("skill dir should be created");
        fs::write(
            root.join("skills").join("demo").join("SKILL.md"),
            "---\ndescription: Demo route skill\n---\n# Demo",
        )
        .expect("skill file should be written");

        let guards = vec![
            EnvVarGuard::set("USERPROFILE", root.as_path()),
            EnvVarGuard::set("HOME", root.as_path()),
            EnvVarGuard::set("LY_SKILLS_DIR", root.join("skills").as_path()),
            EnvVarGuard::set("LY_WORKSPACE_ROOT", root.as_path()),
            EnvVarGuard::set("LY_TOOL_STATE_PATH", root.join("tool-state.json").as_path()),
        ];
        Self { root, guards }
    }

    fn mcp_config_path(&self) -> PathBuf {
        self.root.join("mcp-servers.json")
    }

    fn tool_state_path(&self) -> PathBuf {
        self.root.join("tool-state.json")
    }
}

impl Drop for CapabilityRouteTestEnv {
    fn drop(&mut self) {
        self.guards.clear();
        let _ = fs::remove_dir_all(self.root.as_path());
    }
}

struct PluginCapabilityRouteTestEnv {
    root: PathBuf,
    guards: Vec<EnvVarGuard>,
}

impl PluginCapabilityRouteTestEnv {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rsliteyuki-plugin-capability-route-{nanos}"));
        fs::create_dir_all(root.as_path()).expect("temp plugin capability dir should exist");

        let config_path = root.join("config.yaml");
        fs::write(config_path.as_path(), "core:\n  adapters: []\n")
            .expect("route test config should be written");

        let plugin_dir = root.join("plugins").join("capability_route_plugin");
        fs::create_dir_all(plugin_dir.as_path()).expect("plugin dir should be created");
        fs::write(
                plugin_dir.join("capability_route_plugin.py"),
                r#"from astrbot.api import FunctionTool, star
from astrbot.api.event import filter
from quart import jsonify, make_response, request

CRON_RUNS = []


class CapabilityRoutePlugin(star.Star):
    async def initialize(self):
        self.context.register_web_api("/cap-route", self.handle_api, ["POST"], "cap route api")
        self.context.register_web_api("/cap-route", self.handle_multi_api, ["PATCH"], "cap route patch api")
        self.context.register_web_api("/cap-multi", self.handle_multi_api, ["GET", "PUT", "PATCH", "DELETE"], "cap multi api")
        self.context.register_web_api("/cap-quart", self.handle_quart_api, ["POST"], "cap quart api")
        self.context.register_web_api("/cap-text", self.handle_text_api, ["GET"], "cap text api")
        self.context.register_web_api("/cap-cron-state", self.handle_cron_state, ["GET"], "cap cron state api")
        self.context.add_llm_tools(
            FunctionTool(
                name="manual_capability_route_tool",
                description="manual route tool",
                parameters={"type": "object", "properties": {}},
                handler=self.manual_tool,
            ),
        )
        await self.context.cron_manager.add_active_job(
            name="capability-route-cron",
            description="capability route cron",
            cron_expression="*/5 * * * *",
            payload={"mode": "route"},
            enabled=True,
        )
        await self.context.cron_manager.add_basic_job(
            name="capability-basic-cron",
            handler=self.handle_basic_cron,
            cron_expression="*/5 * * * *",
            description="capability basic cron",
            payload={"mode": "basic"},
            enabled=True,
        )
        self.context.register_task("capability-route-task", "route task")

    async def handle_api(self, request=None):
        if request and request.get("query", {}).get("mode") == "fail":
            raise RuntimeError("capability route web api failed")
        return {
            "status": 201,
            "body": {
                "method": request.get("method") if request else None,
                "path": request.get("path") if request else None,
                "query": request.get("query") if request else None,
                "header": request.get("headers", {}).get("X-Test") if request else None,
                "bodyJson": request.get("bodyJson") if request else None,
                "bodyText": request.get("bodyText") if request else None,
                "bodyBytesBase64": request.get("bodyBytesBase64") if request else None,
                "peerIp": request.get("peerIp") if request else None,
            },
        }

    async def handle_multi_api(self, request=None):
        method = request.get("method") if request else None
        return {
            "status": 202,
            "body": {
                "method": method,
                "query": request.get("query") if request else None,
            },
        }

    async def handle_quart_api(self):
        payload = await request.get_json()
        return make_response(
            jsonify(
                {
                    "method": request.method,
                    "path": request.path,
                    "page": request.args.get("page", 1, type=int),
                    "header": request.headers.get("x-test"),
                    "bodyValue": payload.get("value") if payload else None,
                    "peerIp": request.remote_addr,
                }
            ),
            207,
        )

    async def handle_text_api(self, request=None):
        return "capability route text"

    async def handle_cron_state(self, request=None):
        return {"runs": list(CRON_RUNS)}

    async def handle_basic_cron(self, mode: str = "basic"):
        CRON_RUNS.append(mode)
        return {"mode": mode}

    async def manual_tool(self, value: str = "ok", count: int = 1):
        if value == "fail":
            raise RuntimeError("capability route tool failed")
        return {"value": value, "count": count}

    @filter.llm_tool("decorated_capability_route_tool")
    async def decorated_tool(self, query: str):
        if query == "fail":
            raise RuntimeError("decorated capability route tool failed")
        return query
"#,
            )
            .expect("plugin module should be written");
        fs::write(
            plugin_dir.join("plugin.json"),
            r#"{
  "id": "capability-route-plugin",
  "name": "Capability Route Plugin",
  "type": "service",
  "runtime": {
    "kind": "python",
    "entrypoint": "capability_route_plugin"
  }
}"#,
        )
        .expect("plugin manifest should be written");

        let guards = vec![
            EnvVarGuard::set("LY_CONFIG_PATH", config_path.as_path()),
            EnvVarGuard::set("LY_LLM_CONFIG_PATH", root.join("llm-config.yaml").as_path()),
            EnvVarGuard::set("LY_PASSWORD_PATH", root.join("password.yaml").as_path()),
            EnvVarGuard::set("LY_PLUGIN_DIRS", root.join("plugins").as_path()),
            EnvVarGuard::set("USERPROFILE", root.as_path()),
            EnvVarGuard::set("HOME", root.as_path()),
            EnvVarGuard::set(
                "LY_PLUGIN_CRON_STATE_PATH",
                root.join("plugin-cron-state.json").as_path(),
            ),
        ];
        Self { root, guards }
    }
}

impl Drop for PluginCapabilityRouteTestEnv {
    fn drop(&mut self) {
        self.guards.clear();
        let _ = fs::remove_dir_all(self.root.as_path());
    }
}

fn split_response(response: Vec<u8>) -> (String, Vec<u8>) {
    let Some(split_at) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        panic!("response did not include header separator");
    };
    let body_start = split_at + 4;
    let headers =
        String::from_utf8(response[..body_start].to_vec()).expect("headers should be valid utf8");
    let body = response[body_start..].to_vec();
    (headers, body)
}

fn assert_json_number_close(value: &serde_json::Value, expected: f64) {
    let actual = value
        .as_f64()
        .expect("json value should be a floating-point number");
    assert!(
        (actual - expected).abs() < 0.001,
        "expected {expected}, got {actual}"
    );
}

fn local_auth_header(server: &WebHostService) -> String {
    format!(
        "Authorization: Bearer {}",
        server.auth.local_session_token()
    )
}

fn route_json_api(
    server: &WebHostService,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> serde_json::Value {
    let body_text = body.map(ToString::to_string).unwrap_or_default();
    let request = if body_text.is_empty() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
            local_auth_header(server)
        )
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            local_auth_header(server),
            body_text.len(),
            body_text
        )
    };
    let response = server.route_http_request(request.as_bytes(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    let (headers, body) = split_response(response);
    assert!(
        headers.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected headers for {method} {path}: {headers}"
    );
    serde_json::from_slice(&body).expect("API response body should be valid json")
}

struct TestHttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    json: Value,
}

async fn spawn_mock_capability_mcp_server() -> Result<(String, tokio::task::JoinHandle<()>), String>
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| format!("bind failed: {err}"))?;
    let address = listener
        .local_addr()
        .map_err(|err| format!("addr failed: {err}"))?;
    let handle = tokio::spawn(async move {
        for _ in 0..6 {
            let (mut socket, _) = listener.accept().await.expect("accept should succeed");
            let request = read_test_http_request_json(&mut socket)
                .await
                .expect("request should parse");
            let method = request
                .get("method")
                .and_then(Value::as_str)
                .expect("method should exist");
            match method {
                "initialize" => {
                    write_test_json_response(
                        &mut socket,
                        200,
                        Some("capability-session"),
                        &serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request["id"],
                            "result": {
                                "protocolVersion": "2024-11-05",
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
                    write_test_empty_response(&mut socket, 202)
                        .await
                        .expect("notification response should write");
                }
                "tools/list" => {
                    write_test_json_response(
                        &mut socket,
                        200,
                        None,
                        &serde_json::json!({
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
                other => panic!("unexpected MCP method: {other}"),
            }
        }
    });

    Ok((format!("http://{address}/mcp"), handle))
}

async fn spawn_mock_anthropic_chat_server(
    base_path: &str,
) -> Result<(String, tokio::task::JoinHandle<()>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| format!("bind failed: {err}"))?;
    let address = listener
        .local_addr()
        .map_err(|err| format!("addr failed: {err}"))?;
    let base_path = format!("/{}", base_path.trim_matches('/'));
    let expected_path = format!("{base_path}/v1/messages");
    let response_base_url = format!("http://{address}{base_path}");
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept should succeed");
        let request = read_test_http_request(&mut socket)
            .await
            .expect("request should parse");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, expected_path);
        assert_eq!(
            request.headers.get("x-api-key").map(String::as_str),
            Some("anthropic-test-key")
        );
        assert_eq!(
            request.headers.get("anthropic-version").map(String::as_str),
            Some("2023-06-01")
        );
        assert_eq!(request.json["model"], "claude-sonnet-4-20250514");
        assert_eq!(
            request.json["messages"][0]["content"][0]["text"],
            "hello from custom gateway"
        );
        write_test_json_response(
            &mut socket,
            200,
            None,
            &serde_json::json!({
                "id": "msg_1",
                "model": "claude-sonnet-4-20250514",
                "content": [
                    {
                        "type": "text",
                        "text": "mock anthropic reply"
                    }
                ]
            }),
        )
        .await
        .expect("anthropic response should write");
    });

    Ok((response_base_url, handle))
}

async fn spawn_mock_openai_plugin_tool_server(
    tool_name: &'static str,
) -> Result<(String, tokio::task::JoinHandle<()>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| format!("bind failed: {err}"))?;
    let address = listener
        .local_addr()
        .map_err(|err| format!("addr failed: {err}"))?;
    let response_base_url = format!("http://{address}");

    let handle = tokio::spawn(async move {
        let (mut first_socket, _) = listener.accept().await.expect("accept should succeed");
        let first_request = read_test_http_request(&mut first_socket)
            .await
            .expect("first request should parse");
        assert_eq!(first_request.method, "POST");
        assert_eq!(first_request.path, "/v1/responses");
        assert!(
            first_request
                .json
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| tools
                    .iter()
                    .any(|tool| { tool["name"].as_str() == Some(tool_name) })),
            "first request should advertise plugin runtime tools: {}",
            first_request.json
        );
        write_test_json_response(
            &mut first_socket,
            200,
            None,
            &serde_json::json!({
                "id": "resp_plugin_tool_1",
                "output": [
                    {
                        "id": "item_plugin_tool_1",
                        "type": "function_call",
                        "call_id": "call_plugin_tool_1",
                        "name": tool_name,
                        "arguments": "{\"value\":\"demo\",\"count\":2}"
                    }
                ]
            }),
        )
        .await
        .expect("first response should write");

        let (mut second_socket, _) = listener.accept().await.expect("accept should succeed");
        let second_request = read_test_http_request(&mut second_socket)
            .await
            .expect("second request should parse");
        assert_eq!(second_request.method, "POST");
        assert_eq!(second_request.path, "/v1/responses");
        assert_eq!(
            second_request.json["previous_response_id"],
            "resp_plugin_tool_1"
        );
        assert!(
            second_request
                .json
                .get("input")
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().any(|item| {
                    item["type"].as_str() == Some("function_call_output")
                        && item["call_id"].as_str() == Some("call_plugin_tool_1")
                        && item["output"].as_str().is_some_and(|output| {
                            output.contains("\"value\":\"demo\"") && output.contains("\"count\":2")
                        })
                })),
            "second request should include plugin tool output: {}",
            second_request.json
        );
        write_test_json_response(
            &mut second_socket,
            200,
            None,
            &serde_json::json!({
                "id": "resp_plugin_tool_2",
                "output_text": "plugin tool loop complete"
            }),
        )
        .await
        .expect("second response should write");
    });

    Ok((response_base_url, handle))
}

async fn read_test_http_request(socket: &mut TcpStream) -> Result<TestHttpRequest, String> {
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
        if let Some(headers_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers_text = String::from_utf8_lossy(&buffer[..headers_end]).to_string();
            let mut header_lines = headers_text.lines();
            let request_line = header_lines
                .next()
                .ok_or_else(|| "request did not include a request line".to_string())?;
            let mut request_parts = request_line.split_whitespace();
            let method = request_parts
                .next()
                .ok_or_else(|| "request line missing method".to_string())?
                .to_string();
            let path = request_parts
                .next()
                .ok_or_else(|| "request line missing path".to_string())?
                .to_string();
            let headers = header_lines
                .filter_map(|line| {
                    line.split_once(':').map(|(name, value)| {
                        (name.trim().to_ascii_lowercase(), value.trim().to_string())
                    })
                })
                .collect::<HashMap<_, _>>();
            let content_length = headers
                .get("content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let body_start = headers_end + 4;
            if buffer.len() >= body_start + content_length {
                let body = &buffer[body_start..body_start + content_length];
                let json = serde_json::from_slice(body)
                    .map_err(|err| format!("body decode failed: {err}"))?;
                return Ok(TestHttpRequest {
                    method,
                    path,
                    headers,
                    json,
                });
            }
        }
    }

    Err("request closed before full body was received".to_string())
}

async fn read_test_http_request_json(socket: &mut TcpStream) -> Result<Value, String> {
    Ok(read_test_http_request(socket).await?.json)
}

async fn write_test_json_response(
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

async fn write_test_empty_response(socket: &mut TcpStream, status: u16) -> Result<(), String> {
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

fn bootstrap_login_hash(server: &WebHostService) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(format!("{}.napcat", server.auth.bootstrap_login_token()));
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn root_route_redirects_to_webui_prefix() {
    let response = test_server().route_http_request(
        b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);

    assert!(headers.starts_with("HTTP/1.1 307 Temporary Redirect\r\n"));
    assert!(headers.contains("Location: /webui/\r\n"));
    assert_eq!(
        String::from_utf8(body).expect("body should be utf8"),
        "redirecting"
    );
}

#[test]
fn health_route_returns_runtime_metadata() {
    let response = test_server().route_http_request(
        b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("health body should be valid json");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body["runtime"]["runtime_target"], "tauri2");
    assert_eq!(body["runtime"]["status"], "running");
    assert_eq!(body["runtime"]["adapter_count"], 3);
    assert_json_number_close(
        &body["runtime"]["resource_usage"]["cpu"]["system_percent"],
        63.2,
    );
    assert_eq!(body["bind"], "0.0.0.0:14500");
    assert_eq!(body["desktop_url"], "http://127.0.0.1:14500/");
}

#[test]
fn protected_api_requires_valid_bearer_token() {
    let response = test_server().route_http_request(
        b"GET /api/QQLogin/GetQQLoginInfo HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("unauthorized body should be valid json");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body["code"], 401);
    assert_eq!(body["message"], "Unauthorized");
}

#[test]
fn llm_manager_routes_save_read_fetch_preview_and_test_models() {
    let _lock = env_lock_guard();
    let env = LlmManagerRouteTestEnv::new();
    let server = test_server();

    let save_body = serde_json::json!({
        "activeProviderId": "anthropic-main",
        "providers": [
            {
                "id": "openai-main",
                "label": "OpenAI Main",
                "providerId": "openai",
                "baseUrl": "http://127.0.0.1:9/v1",
                "apiKey": "openai-test-key",
                "timeoutSeconds": 4,
                "headers": { "X-Test": "openai" },
                "models": [
                    { "id": "gpt-5-mini", "enabled": true },
                    { "id": "gpt-4.1", "enabled": false }
                ]
            },
            {
                "id": "anthropic-main",
                "label": "Anthropic Main",
                "providerId": "anthropic",
                "baseUrl": "http://127.0.0.1:9",
                "apiKey": "anthropic-test-key",
                "timeoutSeconds": 5,
                "headers": { "X-Test": "anthropic" },
                "models": [
                    { "id": "claude-sonnet-4-20250514", "enabled": true },
                    { "id": "claude-3-7-sonnet-20250219", "enabled": false }
                ]
            }
        ]
    });

    let save_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/SaveManagerState",
        Some(&save_body),
    );
    assert_eq!(save_response["code"], 0);
    assert_eq!(save_response["data"]["activeProviderId"], "anthropic-main");
    assert_eq!(save_response["data"]["providers"][1]["active"], true);

    let settings_response = route_json_api(&server, "GET", "/api/LLM/GetSettings", None);
    assert_eq!(settings_response["code"], 0);
    let provider_options = settings_response["data"]["providerOptions"]
        .as_array()
        .expect("providerOptions should be an array");
    let openai_option = provider_options
        .iter()
        .find(|provider| provider["baseUrl"] == "http://127.0.0.1:9/v1")
        .expect("openai provider option should be returned");
    assert_eq!(openai_option["label"], "OpenAI Main");
    let anthropic_option = provider_options
        .iter()
        .find(|provider| provider["baseUrl"] == "http://127.0.0.1:9")
        .expect("anthropic provider option should be returned");
    assert_eq!(anthropic_option["label"], "Anthropic Main");

    let state_response = route_json_api(&server, "GET", "/api/LLM/GetManagerState", None);
    assert_eq!(state_response["code"], 0);
    assert_eq!(state_response["data"]["activeProviderId"], "anthropic-main");
    assert_eq!(
        state_response["data"]["providers"]
            .as_array()
            .expect("providers should be an array")
            .len(),
        2
    );
    let active_provider = state_response["data"]["providers"]
        .as_array()
        .expect("providers should be an array")
        .iter()
        .find(|provider| provider["id"] == "anthropic-main")
        .expect("active provider should be returned")
        .clone();
    assert_eq!(active_provider["label"], "Anthropic Main");
    assert_eq!(active_provider["providerLabel"], "Anthropic");

    let fetch_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/FetchModels",
        Some(&serde_json::json!({ "provider": active_provider.clone() })),
    );
    assert_eq!(fetch_response["code"], 0);
    assert_eq!(fetch_response["data"]["source"], "catalog");
    assert!(
        fetch_response["data"]["models"]
            .as_array()
            .is_some_and(|models| models
                .iter()
                .any(|model| model["id"] == "claude-sonnet-4-20250514")),
        "expected anthropic catalog models, got {fetch_response:?}"
    );

    let preview_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/PreviewRequest",
        Some(&serde_json::json!({
            "provider": active_provider.clone(),
            "modelId": "claude-sonnet-4-20250514"
        })),
    );
    assert_eq!(preview_response["code"], 0);
    assert_eq!(
        preview_response["data"]["endpoint"],
        "http://127.0.0.1:9/v1/messages"
    );
    assert_eq!(
        preview_response["data"]["headers"]["anthropic-version"],
        "2023-06-01"
    );
    assert_eq!(preview_response["data"]["headers"]["X-Test"], "anthropic");
    assert!(
        preview_response["data"]["bodyText"]
            .as_str()
            .is_some_and(|body| body.contains("<redacted>")),
        "preview body should be redacted, got {preview_response:?}"
    );

    let test_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/TestModels",
        Some(&serde_json::json!({
            "provider": {
                "id": "fast-fail",
                "label": "Fast Fail",
                "providerId": "openai",
                "baseUrl": "",
                "models": [{ "id": "gpt-5-mini", "enabled": true }]
            },
            "modelId": "gpt-5-mini"
        })),
    );
    assert_eq!(test_response["code"], 0);
    assert_eq!(test_response["data"]["results"][0]["modelId"], "gpt-5-mini");
    assert_eq!(test_response["data"]["results"][0]["ok"], false);
    assert!(
        test_response["data"]["results"][0]["error"]
            .as_str()
            .is_some_and(|error| error.contains("base_url is empty")),
        "expected validation error in model test result, got {test_response:?}"
    );

    let llm_config = fs::read_to_string(env.llm_config_path())
        .expect("LLM manager save should persist llm-config.yaml");
    assert!(llm_config.contains("active_provider_id: 'anthropic-main'"));
    assert!(llm_config.contains("provider: 'anthropic'"));
    assert!(llm_config.contains("base_url: 'http://127.0.0.1:9'"));
    assert!(llm_config.contains("model: 'claude-sonnet-4-20250514'"));
    assert!(llm_config.contains("api_key: 'openai-test-key'"));
    assert!(llm_config.contains("api_key: 'anthropic-test-key'"));
}

#[test]
fn llm_prompt_profile_routes_support_full_management_cycle() {
    let _lock = env_lock_guard();
    let env = LlmManagerRouteTestEnv::new();
    let server = test_server();

    let initial_response = route_json_api(&server, "GET", "/api/LLM/PromptProfiles", None);
    assert_eq!(initial_response["code"], 0);
    assert_eq!(initial_response["data"]["activeProfile"], "default");
    assert_eq!(
        initial_response["data"]["profiles"]
            .as_array()
            .map(|profiles| profiles.len()),
        Some(1)
    );
    assert_eq!(initial_response["data"]["profiles"][0]["name"], "default");
    assert_eq!(initial_response["data"]["profiles"][0]["active"], true);
    assert_eq!(initial_response["data"]["profiles"][0]["canDelete"], false);

    let save_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/PromptProfiles/Save",
        Some(&serde_json::json!({
            "name": " roleplay ",
            "soul": " answer tersely ",
            "active": true,
        })),
    );
    assert_eq!(save_response["code"], 0);
    assert_eq!(save_response["data"]["activeProfile"], "roleplay");
    assert!(
        save_response["data"]["profiles"]
            .as_array()
            .is_some_and(|profiles| profiles.iter().any(|profile| {
                profile["name"] == "roleplay"
                    && profile["soul"] == "answer tersely"
                    && profile["active"] == true
                    && profile["canDelete"] == true
            }))
    );

    let preview_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/PromptProfiles/Preview",
        Some(&serde_json::json!({
            "name": "roleplay",
            "userPrompt": "hello",
        })),
    );
    assert_eq!(preview_response["code"], 0);
    assert_eq!(preview_response["data"]["profile"]["name"], "roleplay");
    assert_eq!(
        preview_response["data"]["preview"]["composedUserPrompt"],
        "answer tersely\n\nhello"
    );
    assert!(
        preview_response["data"]["preview"]["combinedPrompt"]
            .as_str()
            .is_some_and(|value| value.contains("answer tersely\n\nhello"))
    );

    let use_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/PromptProfiles/Use",
        Some(&serde_json::json!({
            "name": "default",
        })),
    );
    assert_eq!(use_response["code"], 0);
    assert_eq!(use_response["data"]["activeProfile"], "default");

    let delete_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/PromptProfiles/Delete",
        Some(&serde_json::json!({
            "name": "roleplay",
        })),
    );
    assert_eq!(delete_response["code"], 0);
    assert_eq!(delete_response["data"]["activeProfile"], "default");
    assert_eq!(
        delete_response["data"]["profiles"]
            .as_array()
            .map(|profiles| profiles.len()),
        Some(1)
    );

    let delete_default_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/PromptProfiles/Delete",
        Some(&serde_json::json!({
            "name": "default",
        })),
    );
    assert_eq!(delete_default_response["code"], -1);
    assert!(
        delete_default_response["message"]
            .as_str()
            .is_some_and(|message| message.contains("default profile cannot be removed"))
    );

    let prompt_store = fs::read_to_string(env.root.join("llm-prompts.json"))
        .expect("prompt profile routes should persist llm-prompts.json");
    assert!(prompt_store.contains("\"active_profile\": \"default\""));
    assert!(prompt_store.contains("\"name\": \"default\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn capability_routes_list_tools_skills_and_mcp_servers() {
    let _lock = env_lock_guard();
    let env = CapabilityRouteTestEnv::new();
    let (url, server_task) = spawn_mock_capability_mcp_server()
        .await
        .expect("mock server should start");
    fs::write(
        env.mcp_config_path(),
        serde_json::json!([
            {
                "name": "mock",
                "url": url,
                "active": true,
                "transport": "streamable_http"
            }
        ])
        .to_string(),
    )
    .expect("mcp config should be written");
    let _mcp_guard = EnvVarGuard::set("LY_MCP_CONFIG_PATH", env.mcp_config_path().as_path());

    let server = test_server();

    let tools_response = route_json_api(&server, "GET", "/api/tools", None);
    assert_eq!(tools_response["code"], 0);
    assert!(
        tools_response["data"]["tools"]
            .as_array()
            .is_some_and(|tools| tools
                .iter()
                .any(|tool| tool["name"] == "workspace_read_file"))
    );
    assert!(
        tools_response["data"]["tools"]
            .as_array()
            .is_some_and(|tools| tools
                .iter()
                .any(|tool| tool["name"] == "list_tool_categories"))
    );

    let skills_response = route_json_api(&server, "GET", "/api/skills", None);
    assert_eq!(skills_response["code"], 0);
    assert_eq!(skills_response["data"]["skills"][0]["name"], "demo");
    assert_eq!(
        skills_response["data"]["skills"][0]["description"],
        "Demo route skill"
    );

    let mcp_response = route_json_api(&server, "GET", "/api/mcp/servers", None);
    assert_eq!(mcp_response["code"], 0);
    assert_eq!(mcp_response["data"]["servers"][0]["name"], "mock");
    assert_eq!(mcp_response["data"]["servers"][0]["toolCount"], 1);
    assert_eq!(
        mcp_response["data"]["servers"][0]["toolNames"][0],
        "lookup_weather"
    );

    server_task.await.expect("mock server should finish");
}

#[tokio::test(flavor = "multi_thread")]
async fn capability_routes_support_mcp_save_test_and_skill_read_upload() {
    let _lock = env_lock_guard();
    let env = CapabilityRouteTestEnv::new();
    let (url, server_task) = spawn_mock_capability_mcp_server()
        .await
        .expect("mock server should start");
    let _mcp_guard = EnvVarGuard::set("LY_MCP_CONFIG_PATH", env.mcp_config_path().as_path());
    let server = test_server();

    let save_response = route_json_api(
        &server,
        "POST",
        "/api/mcp/save",
        Some(&serde_json::json!({
            "servers": [
                {
                    "name": "mock",
                    "url": url,
                    "active": true,
                    "transport": "streamable_http"
                }
            ]
        })),
    );
    assert_eq!(save_response["code"], 0);
    assert_eq!(save_response["data"]["servers"][0]["toolCount"], 1);
    let mcp_config = fs::read_to_string(env.mcp_config_path())
        .expect("mcp/save should persist the MCP config file");
    assert!(mcp_config.contains("\"transport\": \"streamable_http\""));

    let test_response = route_json_api(
        &server,
        "POST",
        "/api/mcp/test",
        Some(&serde_json::json!({
            "server": {
                "name": "mock-test",
                "url": save_response["data"]["servers"][0]["url"],
                "active": true,
                "transport": "streamable_http"
            }
        })),
    );
    assert_eq!(test_response["code"], 0);
    assert_eq!(test_response["data"]["servers"][0]["toolCount"], 1);
    assert_eq!(
        test_response["data"]["servers"][0]["toolNames"][0],
        "lookup_weather"
    );

    let read_response = route_json_api(&server, "GET", "/api/skills/read?name=demo", None);
    assert_eq!(read_response["code"], 0);
    assert_eq!(read_response["data"]["name"], "demo");
    assert_eq!(read_response["data"]["path"], "skills/demo/SKILL.md");
    assert!(
        read_response["data"]["content"]
            .as_str()
            .is_some_and(|content| content.contains("# Demo"))
    );

    let upload_response = route_json_api(
        &server,
        "POST",
        "/api/skills/upload",
        Some(&serde_json::json!({
            "name": "uploaded-skill",
            "content": "---\ndescription: Uploaded skill\n---\n# Uploaded"
        })),
    );
    assert_eq!(upload_response["code"], 0);
    assert_eq!(upload_response["data"]["name"], "uploaded-skill");
    assert_eq!(
        upload_response["data"]["path"],
        "skills/uploaded-skill/SKILL.md"
    );
    let uploaded_skill = fs::read_to_string(
        env.root
            .join("skills")
            .join("uploaded-skill")
            .join("SKILL.md"),
    )
    .expect("skills/upload should persist the SKILL.md file");
    assert!(uploaded_skill.contains("description: Uploaded skill"));

    let skills_response = route_json_api(&server, "GET", "/api/skills", None);
    assert_eq!(skills_response["code"], 0);
    assert!(
        skills_response["data"]["skills"]
            .as_array()
            .is_some_and(|skills| skills.iter().any(|skill| skill["name"] == "uploaded-skill"))
    );

    server_task.await.expect("mock server should finish");
}

#[test]
fn capability_routes_support_tool_toggle_and_persist_state() {
    let _lock = env_lock_guard();
    let env = CapabilityRouteTestEnv::new();
    let server = test_server();

    let initial_response = route_json_api(&server, "GET", "/api/tools", None);
    assert_eq!(initial_response["code"], 0);
    assert!(
        initial_response["data"]["tools"]
            .as_array()
            .is_some_and(|tools| tools
                .iter()
                .any(|tool| { tool["name"] == "workspace_read_file" && tool["active"] == true }))
    );

    let toggle_response = route_json_api(
        &server,
        "POST",
        "/api/tools/toggle",
        Some(&serde_json::json!({
            "name": "workspace_read_file",
            "active": false
        })),
    );
    assert_eq!(toggle_response["code"], 0);
    assert_eq!(toggle_response["data"]["name"], "workspace_read_file");
    assert_eq!(toggle_response["data"]["active"], false);
    assert!(
        toggle_response["data"]["configPath"]
            .as_str()
            .is_some_and(|path| path.ends_with("tool-state.json"))
    );
    assert!(
        toggle_response["data"]["tools"]
            .as_array()
            .is_some_and(|tools| tools
                .iter()
                .any(|tool| { tool["name"] == "workspace_read_file" && tool["active"] == false }))
    );

    let refreshed_response = route_json_api(&server, "GET", "/api/tools", None);
    assert_eq!(refreshed_response["code"], 0);
    assert!(
        refreshed_response["data"]["tools"]
            .as_array()
            .is_some_and(|tools| tools
                .iter()
                .any(|tool| { tool["name"] == "workspace_read_file" && tool["active"] == false }))
    );

    let tool_state = fs::read_to_string(env.tool_state_path())
        .expect("tools/toggle should persist tool-state.json");
    assert!(tool_state.contains("\"workspace_read_file\": false"));
}

#[test]
fn capability_routes_reject_discovery_helper_toggle_and_invalid_state_overwrite() {
    let _lock = env_lock_guard();
    let env = CapabilityRouteTestEnv::new();
    let server = test_server();

    let helper_response = route_json_api(
        &server,
        "POST",
        "/api/tools/toggle",
        Some(&serde_json::json!({
            "name": "list_tool_categories",
            "active": false
        })),
    );
    assert_eq!(helper_response["code"], -1);
    assert!(
        helper_response["message"]
            .as_str()
            .is_some_and(|message| message.contains("required discovery helper"))
    );

    fs::write(env.tool_state_path(), "{invalid json").expect("broken tool state should be written");
    let invalid_response = route_json_api(
        &server,
        "POST",
        "/api/tools/toggle",
        Some(&serde_json::json!({
            "name": "workspace_read_file",
            "active": false
        })),
    );
    assert_eq!(invalid_response["code"], -1);
    assert!(
        invalid_response["message"]
            .as_str()
            .is_some_and(|message| message.contains("invalid json"))
    );
    let persisted = fs::read_to_string(env.tool_state_path())
        .expect("invalid tool state should not be overwritten");
    assert_eq!(persisted, "{invalid json");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_capability_routes_expose_runtime_snapshot_queries() {
    let _lock = env_lock_guard();
    let env = PluginCapabilityRouteTestEnv::new();
    let runtime_host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
        .await
        .expect("embedded host should start for capability route test");
    let catalog = runtime_host.plugin_catalog_snapshot().await;
    let catalog_entry = catalog
        .entries
        .iter()
        .find(|entry| entry.descriptor.metadata.id == "capability-route-plugin")
        .expect("plugin should be discovered in embedded host catalog");
    assert!(
        catalog_entry.loaded,
        "plugin should be loaded before querying capability routes"
    );
    let direct_snapshot = runtime_host
        .plugin_capability_snapshot("capability-route-plugin")
        .await
        .expect("direct capability snapshot query should succeed");
    assert!(
        direct_snapshot.is_some(),
        "python capability snapshot should exist for embedded host route test"
    );
    let server = test_server().with_runtime_host(runtime_host.clone());

    let capabilities_response = route_json_api(
        &server,
        "GET",
        "/api/Plugin/Capabilities?id=capability-route-plugin",
        None,
    );
    assert_eq!(capabilities_response["code"], 0);
    assert_eq!(
        capabilities_response["data"]["pluginId"],
        "capability-route-plugin"
    );
    assert_eq!(capabilities_response["data"]["runtimeKind"], "python");
    assert_eq!(
        capabilities_response["data"]["support"]["tools"]["registered"],
        true
    );
    assert_eq!(
        capabilities_response["data"]["support"]["tools"]["executable"],
        true
    );
    assert_eq!(
        capabilities_response["data"]["support"]["tools"]["status"],
        "active"
    );
    assert_eq!(
        capabilities_response["data"]["support"]["cronJobs"]["registered"],
        true
    );
    assert_eq!(
        capabilities_response["data"]["support"]["cronJobs"]["executable"],
        true
    );
    assert_eq!(
        capabilities_response["data"]["support"]["cronJobs"]["status"],
        "active"
    );
    assert_eq!(
        capabilities_response["data"]["snapshot"]["tools"]
            .as_array()
            .map(|items| items.len()),
        Some(2)
    );
    assert_eq!(
        capabilities_response["data"]["snapshot"]["webApis"][0]["route"],
        "/cap-route"
    );
    assert_eq!(
        capabilities_response["data"]["snapshot"]["cronJobs"]
            .as_array()
            .map(|items| items.len()),
        Some(2)
    );
    assert!(
        capabilities_response["data"]["snapshot"]["cronJobs"]
            .as_array()
            .is_some_and(|items| items.iter().any(|job| {
                job["jobType"] == "active_agent"
                    && job["jobId"]
                        .as_str()
                        .is_some_and(|id| id.contains("active_agent"))
            }))
    );
    assert!(
        capabilities_response["data"]["snapshot"]["cronJobs"]
            .as_array()
            .is_some_and(|items| items
                .iter()
                .any(|job| { job["jobType"] == "basic" && job["nextRunTime"].is_string() }))
    );
    assert_eq!(
        capabilities_response["data"]["snapshot"]["tasks"][0]["taskId"],
        "capability-route-task"
    );

    let tools_response = route_json_api(
        &server,
        "GET",
        "/api/Plugin/Tools?id=capability-route-plugin",
        None,
    );
    assert_eq!(tools_response["code"], 0);
    assert_eq!(tools_response["data"]["support"]["registered"], true);
    assert_eq!(tools_response["data"]["support"]["executable"], true);
    assert_eq!(
        tools_response["data"]["items"]
            .as_array()
            .map(|items| items.len()),
        Some(2)
    );
    assert!(
        tools_response["data"]["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|tool| {
                tool["name"] == "decorated_capability_route_tool"
                    && tool["source"] == "astrbot_decorator"
            }))
    );

    let all_response = route_json_api(&server, "GET", "/api/Plugin/Capabilities/All", None);
    assert_eq!(all_response["code"], 0);
    assert!(all_response["data"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["pluginId"] == "capability-route-plugin")
    }));

    runtime_host
        .shutdown()
        .await
        .expect("embedded host should shutdown cleanly");
    drop(env);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_tool_execution_and_diagnostics_routes_reflect_runtime_state() {
    let _lock = env_lock_guard();
    let env = PluginCapabilityRouteTestEnv::new();
    let runtime_host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
        .await
        .expect("embedded host should start for plugin diagnostics route test");
    let server = test_server().with_runtime_host(runtime_host.clone());

    let runtime_state = route_json_api(
        &server,
        "GET",
        "/api/Plugin/RuntimeState?id=capability-route-plugin",
        None,
    );
    assert_eq!(runtime_state["code"], 0);
    assert_eq!(runtime_state["data"]["loaded"], true);
    assert_eq!(runtime_state["data"]["enabled"], true);
    assert_eq!(runtime_state["data"]["active"], true);
    assert_eq!(runtime_state["data"]["executableBindings"]["tools"], true);
    assert_eq!(runtime_state["data"]["executableBindings"]["webApis"], true);
    assert_eq!(
        runtime_state["data"]["executableBindings"]["cronJobs"],
        true
    );
    assert_eq!(runtime_state["data"]["schedulerStatus"], "active");

    let execute_response = route_json_api(
        &server,
        "POST",
        "/api/Plugin/Tools/Execute",
        Some(&serde_json::json!({
            "id": "capability-route-plugin",
            "name": "manual_capability_route_tool",
            "arguments": {
                "value": "demo",
                "count": 2
            }
        })),
    );
    assert_eq!(execute_response["code"], 0);
    assert_eq!(
        execute_response["data"]["runtimeName"],
        "plugin::capability-route-plugin::manual_capability_route_tool"
    );
    assert_eq!(execute_response["data"]["output"]["kind"], "json");
    assert_eq!(execute_response["data"]["output"]["value"]["value"], "demo");
    assert_eq!(execute_response["data"]["output"]["value"]["count"], 2);

    let failing_tool_response = route_json_api(
        &server,
        "POST",
        "/api/Plugin/Tools/Execute",
        Some(&serde_json::json!({
            "id": "capability-route-plugin",
            "name": "manual_capability_route_tool",
            "arguments": {
                "value": "fail"
            }
        })),
    );
    assert_eq!(failing_tool_response["code"], -1);
    assert!(
        failing_tool_response["message"]
            .as_str()
            .is_some_and(|message| message.contains("capability route tool failed"))
    );

    let failing_web_api_request = format!(
        "POST /api/Plugin/Runtime/WebApi/capability-route-plugin/cap-route?mode=fail HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Length: 0\r\n\r\n",
        local_auth_header(&server)
    );
    let failing_web_api_response = server.route_http_request(
        failing_web_api_request.as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (failing_headers, failing_body) = split_response(failing_web_api_response);
    let failing_body = String::from_utf8(failing_body).expect("failure body should be utf8");
    assert!(failing_headers.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
    assert!(failing_body.contains("capability route web api failed"));

    let diagnostics = route_json_api(
        &server,
        "GET",
        "/api/Plugin/Diagnostics?id=capability-route-plugin",
        None,
    );
    assert_eq!(diagnostics["code"], 0);
    assert_eq!(diagnostics["data"]["loadState"], "loaded");
    assert_eq!(diagnostics["data"]["executableBindings"]["tools"], true);
    assert_eq!(diagnostics["data"]["executableBindings"]["webApis"], true);
    assert!(
        diagnostics["data"]["lastToolExecution"]["lastError"]
            .as_str()
            .is_some_and(|message| message.contains("capability route tool failed"))
    );
    assert!(
        diagnostics["data"]["lastWebApiDispatch"]["lastError"]
            .as_str()
            .is_some_and(|message| message.contains("capability route web api failed"))
    );
    assert!(
        diagnostics["data"]["lastToolExecution"]["lastSuccessAt"].is_string(),
        "successful tool execution should record lastSuccessAt"
    );

    runtime_host
        .shutdown()
        .await
        .expect("embedded host should shutdown cleanly");
    drop(env);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_cron_scheduler_executes_basic_jobs_and_updates_diagnostics() {
    let _lock = env_lock_guard();
    let env = PluginCapabilityRouteTestEnv::new();
    let runtime_host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
        .await
        .expect("embedded host should start for cron scheduler route test");
    let server = test_server().with_runtime_host(runtime_host.clone());

    let tick_at = chrono::DateTime::parse_from_rfc3339("2026-04-26T10:05:00Z")
        .expect("timestamp should parse")
        .with_timezone(&chrono::Utc);
    let executed = runtime_host
        .run_plugin_cron_tick_at(tick_at)
        .await
        .expect("cron tick should succeed");
    assert_eq!(executed, 1);

    let cron_state_request = format!(
        "GET /api/Plugin/Runtime/WebApi/capability-route-plugin/cap-cron-state HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
        local_auth_header(&server)
    );
    let cron_state_response = server.route_http_request(
        cron_state_request.as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (_, cron_state_body) = split_response(cron_state_response);
    let cron_state_json: serde_json::Value =
        serde_json::from_slice(&cron_state_body).expect("cron state should be json");
    assert_eq!(cron_state_json["runs"][0], "basic");

    let diagnostics = route_json_api(
        &server,
        "GET",
        "/api/Plugin/Diagnostics?id=capability-route-plugin",
        None,
    );
    assert_eq!(diagnostics["code"], 0);
    assert!(
        diagnostics["data"]["lastCronExecution"]["lastSuccessAt"].is_string(),
        "cron execution should record lastSuccessAt"
    );

    let cron_jobs = route_json_api(
        &server,
        "GET",
        "/api/Plugin/CronJobs?id=capability-route-plugin",
        None,
    );
    assert_eq!(cron_jobs["code"], 0);
    assert_eq!(cron_jobs["data"]["support"]["executable"], true);
    let basic_job = cron_jobs["data"]["items"]
        .as_array()
        .and_then(|items| items.iter().find(|job| job["jobType"] == "basic"))
        .expect("cron jobs should include the executable basic job");
    if let Some(value) = basic_job["lastRunTime"].as_str() {
        chrono::DateTime::parse_from_rfc3339(value)
            .expect("basic cron lastRunTime should be valid RFC3339");
    }

    runtime_host
        .shutdown()
        .await
        .expect("embedded host should shutdown cleanly");
    drop(env);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_list_route_includes_runtime_source_and_capability_flags() {
    let _lock = env_lock_guard();
    let env = PluginCapabilityRouteTestEnv::new();
    let runtime_host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
        .await
        .expect("embedded host should start for plugin list route test");
    let server = test_server().with_runtime_host(runtime_host.clone());

    let list_response = route_json_api(&server, "GET", "/api/Plugin/List", None);
    assert_eq!(list_response["code"], 0);

    let plugin = list_response["data"]["plugins"]
        .as_array()
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|plugin| plugin["id"] == "capability-route-plugin")
        })
        .cloned()
        .expect("capability route plugin should appear in plugin list");
    assert_eq!(plugin["runtimeKind"], "python");
    assert_eq!(plugin["pluginType"], "service");
    assert_eq!(plugin["sourceKind"], "astrbot-compatible");
    assert_eq!(plugin["compatKind"], "astrbot");
    assert_eq!(plugin["hasCapabilities"]["any"], true);
    assert_eq!(plugin["hasCapabilities"]["tools"], true);
    assert_eq!(plugin["hasCapabilities"]["webApis"], true);
    assert_eq!(plugin["hasCapabilities"]["cronJobs"], true);
    assert_eq!(plugin["hasCapabilities"]["tasks"], true);
    assert_eq!(plugin["hasPages"], false);
    assert_eq!(plugin["hasConfig"], false);

    runtime_host
        .shutdown()
        .await
        .expect("embedded host should shutdown cleanly");
    drop(env);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn llm_chat_routes_include_plugin_runtime_tools() {
    let _lock = env_lock_guard();
    let env = PluginCapabilityRouteTestEnv::new();
    let runtime_host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
        .await
        .expect("embedded host should start for plugin tool llm chat test");
    let server = test_server().with_runtime_host(runtime_host.clone());
    let (base_url, upstream_task) = spawn_mock_openai_plugin_tool_server(
        "plugin::capability-route-plugin::manual_capability_route_tool",
    )
    .await
    .expect("mock openai plugin tool server should start");

    let save_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/SaveManagerState",
        Some(&serde_json::json!({
            "activeProviderId": "plugin-tool-openai",
            "providers": [
                {
                    "id": "plugin-tool-openai",
                    "label": "Plugin Tool OpenAI",
                    "providerId": "openai-compatible",
                    "baseUrl": base_url,
                    "apiKey": "openai-test-key",
                    "timeoutSeconds": 5,
                    "models": [
                        { "id": "gpt-test", "enabled": true }
                    ]
                }
            ]
        })),
    );
    assert_eq!(save_response["code"], 0);

    let chat_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/Chat",
        Some(&serde_json::json!({
            "message": "run the plugin tool",
            "baseUrl": base_url,
            "model": "gpt-test"
        })),
    );
    assert_eq!(chat_response["code"], 0);
    assert_eq!(
        chat_response["data"]["message"],
        "plugin tool loop complete"
    );
    assert_eq!(chat_response["data"]["baseUrl"], base_url);
    assert_eq!(chat_response["data"]["model"], "gpt-test");

    upstream_task
        .await
        .expect("mock openai plugin tool server should finish");
    runtime_host
        .shutdown()
        .await
        .expect("embedded host should shutdown cleanly");
    drop(env);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_runtime_web_api_routes_dispatch_registered_handlers() {
    let _lock = env_lock_guard();
    let env = PluginCapabilityRouteTestEnv::new();
    let runtime_host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
        .await
        .expect("embedded host should start for runtime web api route test");
    let server = test_server().with_runtime_host(runtime_host.clone());

    let request_body = serde_json::json!({
        "mode": "echo",
        "value": "demo"
    })
    .to_string();
    let request = format!(
        "POST /api/Plugin/Runtime/WebApi/capability-route-plugin/cap-route?foo=bar HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Type: application/json\r\nX-Test: route-header\r\nContent-Length: {}\r\n\r\n{}",
        local_auth_header(&server),
        request_body.len(),
        request_body
    );
    let response = server.route_http_request(request.as_bytes(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    let (headers, body) = split_response(response);
    let body_text = String::from_utf8(body).expect("plugin runtime web api body should be utf8");
    let payload: serde_json::Value = serde_json::from_str(body_text.as_str()).unwrap_or_else(|err| {
            panic!("plugin runtime web api body should be json: {err}; headers={headers}; body={body_text}")
        });

    assert!(headers.starts_with("HTTP/1.1 201 Created\r\n"));
    assert!(headers.contains("Content-Type: application/json; charset=utf-8\r\n"));
    assert_eq!(payload["method"], "POST");
    assert_eq!(payload["path"], "/cap-route");
    assert_eq!(payload["query"]["foo"], "bar");
    assert_eq!(payload["header"], "route-header");
    assert_eq!(payload["bodyJson"]["value"], "demo");
    assert_eq!(payload["peerIp"], "127.0.0.1");
    assert!(
        payload["bodyText"]
            .as_str()
            .is_some_and(|text| text.contains("\"mode\":\"echo\"")),
        "bodyText should contain the serialized request payload: {payload}"
    );
    assert_eq!(
        payload["bodyBytesBase64"],
        "eyJtb2RlIjoiZWNobyIsInZhbHVlIjoiZGVtbyJ9"
    );

    let quart_request_body = serde_json::json!({
        "value": "quart-demo"
    })
    .to_string();
    let quart_request = format!(
        "POST /api/Plugin/Runtime/WebApi/capability-route-plugin/cap-quart?page=7 HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Type: application/json\r\nX-Test: quart-header\r\nContent-Length: {}\r\n\r\n{}",
        local_auth_header(&server),
        quart_request_body.len(),
        quart_request_body
    );
    let quart_response =
        server.route_http_request(quart_request.as_bytes(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    let (quart_headers, quart_body) = split_response(quart_response);
    let quart_body_text =
        String::from_utf8(quart_body).expect("quart compat response body should be utf8");
    let quart_payload: serde_json::Value =
            serde_json::from_str(quart_body_text.as_str()).unwrap_or_else(|err| {
                panic!(
                    "quart compat response body should be json: {err}; headers={quart_headers}; body={quart_body_text}"
                )
            });

    assert!(quart_headers.starts_with("HTTP/1.1 207 "));
    assert!(quart_headers.contains("Content-Type: application/json; charset=utf-8\r\n"));
    assert_eq!(quart_payload["method"], "POST");
    assert_eq!(quart_payload["path"], "/cap-quart");
    assert_eq!(quart_payload["page"], 7);
    assert_eq!(quart_payload["header"], "quart-header");
    assert_eq!(quart_payload["bodyValue"], "quart-demo");
    assert_eq!(quart_payload["peerIp"], "127.0.0.1");

    let text_request = format!(
        "GET /api/Plugin/Runtime/WebApi/capability-route-plugin/cap-text HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
        local_auth_header(&server)
    );
    let text_response =
        server.route_http_request(text_request.as_bytes(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    let (text_headers, text_body) = split_response(text_response);
    let text_body = String::from_utf8(text_body).expect("text response should be utf8");

    assert!(text_headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(text_headers.contains("Content-Type: text/plain; charset=utf-8\r\n"));
    assert_eq!(text_body, "capability route text");

    runtime_host
        .shutdown()
        .await
        .expect("embedded host should shutdown cleanly");
    drop(env);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_runtime_web_api_routes_enforce_methods_and_disable_cleanup() {
    let _lock = env_lock_guard();
    let env = PluginCapabilityRouteTestEnv::new();
    let runtime_host = EmbeddedAppHost::start_for_target(RuntimeTarget::Tauri2)
        .await
        .expect("embedded host should start for runtime web api cleanup test");
    let server = test_server().with_runtime_host(runtime_host.clone());

    let method_mismatch_request = format!(
        "GET /api/Plugin/Runtime/WebApi/capability-route-plugin/cap-route HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
        local_auth_header(&server)
    );
    let method_mismatch_response = server.route_http_request(
        method_mismatch_request.as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (method_headers, method_body) = split_response(method_mismatch_response);
    let method_body = String::from_utf8(method_body).expect("405 body should be utf8");
    assert!(method_headers.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
    assert_eq!(method_body, "method not allowed");

    let patch_request = format!(
        "PATCH /api/Plugin/Runtime/WebApi/capability-route-plugin/cap-route?kind=patch HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Length: 0\r\n\r\n",
        local_auth_header(&server)
    );
    let patch_response =
        server.route_http_request(patch_request.as_bytes(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    let (patch_headers, patch_body) = split_response(patch_response);
    let patch_body_text = String::from_utf8(patch_body).expect("patch body should be utf8");
    let patch_payload: serde_json::Value = serde_json::from_str(patch_body_text.as_str())
        .unwrap_or_else(|err| {
            panic!(
                "patch body should be json: {err}; headers={patch_headers}; body={patch_body_text}"
            )
        });
    assert!(patch_headers.starts_with("HTTP/1.1 202 Accepted\r\n"));
    assert_eq!(patch_payload["method"], "PATCH");
    assert_eq!(patch_payload["query"]["kind"], "patch");

    runtime_host
        .apply_disabled_plugins(vec!["capability-route-plugin".to_string()])
        .await
        .expect("plugin should be disabled through runtime host");

    let disabled_request = format!(
        "POST /api/Plugin/Runtime/WebApi/capability-route-plugin/cap-route HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Length: 0\r\n\r\n",
        local_auth_header(&server)
    );
    let disabled_response =
        server.route_http_request(disabled_request.as_bytes(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    let (disabled_headers, disabled_body) = split_response(disabled_response);
    let disabled_body = String::from_utf8(disabled_body).expect("404 body should be utf8");
    assert!(disabled_headers.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert_eq!(disabled_body, "plugin runtime web api not found");

    runtime_host
        .shutdown()
        .await
        .expect("embedded host should shutdown cleanly");
    drop(env);
}

#[tokio::test(flavor = "multi_thread")]
async fn llm_settings_and_chat_keep_explicit_anthropic_provider_on_custom_gateway() {
    let _lock = env_lock_guard();
    let env = LlmManagerRouteTestEnv::new();
    let server = test_server();
    let (base_url, upstream_task) = spawn_mock_anthropic_chat_server("company-gateway")
        .await
        .expect("mock anthropic server should start");

    let save_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/SaveManagerState",
        Some(&serde_json::json!({
            "activeProviderId": "anthropic-gateway",
            "providers": [
                {
                    "id": "anthropic-gateway",
                    "label": "Anthropic Gateway",
                    "providerId": "anthropic",
                    "baseUrl": base_url,
                    "apiKey": "anthropic-test-key",
                    "timeoutSeconds": 5,
                    "models": [
                        { "id": "claude-sonnet-4-20250514", "enabled": true }
                    ]
                }
            ]
        })),
    );
    assert_eq!(save_response["code"], 0);

    let settings_response = route_json_api(&server, "GET", "/api/LLM/GetSettings", None);
    assert_eq!(settings_response["code"], 0);
    assert_eq!(settings_response["data"]["provider"], "Anthropic Gateway");
    assert_eq!(settings_response["data"]["baseUrl"], base_url);
    assert_eq!(
        settings_response["data"]["providerOptions"][0]["label"],
        "Anthropic Gateway"
    );
    assert_eq!(
        settings_response["data"]["providerOptions"][0]["baseUrl"],
        base_url
    );

    let chat_response = route_json_api(
        &server,
        "POST",
        "/api/LLM/Chat",
        Some(&serde_json::json!({
            "message": "hello from custom gateway",
            "baseUrl": base_url
        })),
    );
    assert_eq!(chat_response["code"], 0);
    assert_eq!(chat_response["data"]["message"], "mock anthropic reply");
    assert_eq!(chat_response["data"]["model"], "claude-sonnet-4-20250514");
    assert_eq!(chat_response["data"]["baseUrl"], base_url);

    let llm_config = fs::read_to_string(env.llm_config_path())
        .expect("custom anthropic config should be persisted");
    assert!(llm_config.contains("provider: 'anthropic'"));
    assert!(llm_config.contains(base_url.as_str()));

    upstream_task
        .await
        .expect("mock anthropic server should finish");
}

#[test]
fn auth_route_supports_bootstrap_then_password_login() {
    let server = test_server();
    let bootstrap_hash = bootstrap_login_hash(&server);
    let bootstrap_body = format!("{{\"hash\":\"{bootstrap_hash}\"}}");
    let login_response = server.route_http_request(
            format!(
                "POST /api/auth/login HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                bootstrap_body.len(),
                bootstrap_body
            )
            .as_bytes(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
    let (_, login_body) = split_response(login_response);
    let login_body: serde_json::Value =
        serde_json::from_slice(&login_body).expect("bootstrap login body should be valid json");
    let issued_session = login_body["data"]["Credential"]
        .as_str()
        .expect("bootstrap login should return a session token");

    let update_body = r#"{"newPassword":"Pass1234"}"#;
    let update_response = server.route_http_request(
            format!(
                "POST /api/auth/update_password HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {issued_session}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                update_body.len(),
                update_body
            )
            .as_bytes(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
    let (_, update_body) = split_response(update_response);
    let update_body: serde_json::Value =
        serde_json::from_slice(&update_body).expect("update password body should be valid json");
    assert_eq!(update_body["code"], 0);
    assert_eq!(update_body["data"], true);

    let password_body = r#"{"password":"Pass1234"}"#;
    let password_response = server.route_http_request(
            format!(
                "POST /api/auth/login/password HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                password_body.len(),
                password_body
            )
            .as_bytes(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
    let (_, password_body) = split_response(password_response);
    let password_body: serde_json::Value =
        serde_json::from_slice(&password_body).expect("password login body should be valid json");
    assert!(
        password_body["data"]["Credential"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    let disabled_response = server.route_http_request(
            format!(
                "POST /api/auth/login HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                bootstrap_body.len(),
                bootstrap_body
            )
            .as_bytes(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
    let (_, disabled_body) = split_response(disabled_response);
    let disabled_body: serde_json::Value = serde_json::from_slice(&disabled_body)
        .expect("disabled bootstrap login body should be valid json");
    assert_ne!(disabled_body["code"], 0);
}

#[test]
fn system_status_route_returns_frontend_compatible_shape() {
    let server = test_server();
    let response = server.route_http_request(
        format!(
            "GET /api/base/GetSysStatusRealTime HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
            local_auth_header(&server)
        )
        .as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body = String::from_utf8(body).expect("sse body should be utf8");
    let payload = body
        .strip_prefix("data: ")
        .and_then(|value| value.strip_suffix("\n\n"))
        .expect("sse body should contain one data event");
    let payload: serde_json::Value =
        serde_json::from_str(payload).expect("system status event should be valid json");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(headers.contains("Content-Type: text/event-stream; charset=utf-8\r\n"));
    assert_json_number_close(&payload["cpu"]["usage"]["system"], 63.2);
    assert_json_number_close(&payload["cpu"]["usage"]["qq"], 18.6);
    assert_eq!(payload["memory"]["total"], 16_384);
    assert_eq!(payload["memory"]["usage"]["system"], 7_168);
    assert_eq!(payload["memory"]["usage"]["qq"], 512);
    assert_eq!(payload["arch"], arch_label());
    assert!(
        payload["cpu"]["core"]
            .as_u64()
            .is_some_and(|value| value >= 1),
        "cpu core count should be populated, got {payload:?}"
    );
    assert!(
        payload["cpu"]["model"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "cpu model should not be empty, got {payload:?}"
    );
}

#[test]
fn qq_login_info_route_uses_runtime_identity() {
    let server = test_server();
    let response = server.route_http_request(
            format!(
                "POST /api/QQLogin/GetQQLoginInfo HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Length: 2\r\n\r\n{{}}",
                local_auth_header(&server)
            )
            .as_bytes(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
    let (headers, body) = split_response(response);
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("qq login info body should be valid json");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["nick"], "Liteyuki");
    assert_eq!(body["data"]["uin"], "local-webui");
    assert_eq!(body["data"]["uid"], "local-webui");
    assert_eq!(body["data"]["online"], true);
    assert!(body["data"]["avatarUrl"].is_null());
}

#[test]
fn theme_css_route_returns_generated_stylesheet() {
    let response = test_server().route_http_request(
        b"GET /files/theme.css HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body = String::from_utf8(body).expect("theme css body should be utf8");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(headers.contains("Content-Type: text/css; charset=utf-8\r\n"));
    assert!(body.contains("--font-family-base"));
}

#[test]
fn public_font_route_serves_builtin_webui_fonts() {
    let response = test_server().route_http_request(
        b"GET /webui/fonts/AaCute.woff HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(headers.contains("Content-Type: font/woff\r\n"));
    assert!(!body.is_empty());
}

#[test]
fn logs_route_returns_recent_buffered_entries() {
    let server = test_server();
    let unique = format!(
        "web-host-log-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos()
    );
    crate::emit_console_log(crate::LogLevel::Info, "web.host.test", unique.as_str());

    let response = server.route_http_request(
        format!(
            "GET /api/logs HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
            local_auth_header(&server)
        )
        .as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("logs body should be valid json");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(
        body["entries"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| entry["message"] == unique)),
        "expected buffered log entry in response, got {body:?}"
    );
}

#[test]
fn terminal_routes_create_list_and_close_sessions() {
    let server = test_server();

    let create_response = server.route_http_request(
            format!(
                "POST /api/Log/terminal/create HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Type: application/json\r\nContent-Length: 21\r\n\r\n{{\"cols\":100,\"rows\":30}}",
                local_auth_header(&server)
            )
            .as_bytes(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
    let (_, create_body) = split_response(create_response);
    let create_body: serde_json::Value =
        serde_json::from_slice(&create_body).expect("create body should be valid json");
    let terminal_id = create_body["data"]["id"]
        .as_str()
        .expect("terminal id should be returned")
        .to_string();
    assert!(terminal_id.starts_with("term-"));

    let list_response = server.route_http_request(
        format!(
            "GET /api/Log/terminal/list HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
            local_auth_header(&server)
        )
        .as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (_, list_body) = split_response(list_response);
    let list_body: serde_json::Value =
        serde_json::from_slice(&list_body).expect("list body should be valid json");
    assert!(
        list_body["data"]
            .as_array()
            .is_some_and(|items| items.iter().any(|entry| entry["id"] == terminal_id)),
        "created terminal should appear in list, got {list_body:?}"
    );

    let close_response = server.route_http_request(
            format!(
                "POST /api/Log/terminal/{terminal_id}/close HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Length: 0\r\n\r\n",
                local_auth_header(&server)
            )
            .as_bytes(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
    let (_, close_body) = split_response(close_response);
    let close_body: serde_json::Value =
        serde_json::from_slice(&close_body).expect("close body should be valid json");
    assert_eq!(close_body["data"], true);

    let list_response = server.route_http_request(
        format!(
            "GET /api/Log/terminal/list HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
            local_auth_header(&server)
        )
        .as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (_, list_body) = split_response(list_response);
    let list_body: serde_json::Value =
        serde_json::from_slice(&list_body).expect("list body should be valid json");
    assert!(
        list_body["data"]
            .as_array()
            .is_some_and(|items| items.iter().all(|entry| entry["id"] != terminal_id)),
        "closed terminal should be removed from list, got {list_body:?}"
    );
}

#[test]
fn appended_log_entries_handles_ring_buffer_rotation() {
    let entry = |line: &str| BufferedLogEntry {
        timestamp: "2026-04-22T00:00:00Z".to_string(),
        level: "INFO".to_string(),
        module: "web.host.test".to_string(),
        message: line.to_string(),
        line: line.to_string(),
    };

    let previous = vec![entry("a"), entry("b"), entry("c")];
    let current = vec![entry("b"), entry("c"), entry("d"), entry("e")];
    let appended = appended_log_entries(previous.as_slice(), current.as_slice());

    assert_eq!(
        appended
            .into_iter()
            .map(|item| item.message)
            .collect::<Vec<_>>(),
        vec!["d".to_string(), "e".to_string()]
    );
}

#[test]
fn terminal_history_replays_only_last_three_lines() {
    let session = TerminalSession::new(
        "term-test".to_string(),
        TERMINAL_DEFAULT_COLS,
        TERMINAL_DEFAULT_ROWS,
        preferred_terminal_shell(),
    );

    session.remember_output("line-1\r\nline-2\r\n");
    session.remember_output("line-3\r\nline-4");

    assert_eq!(
        session.recent_history_text().as_deref(),
        Some("line-2\r\nline-3\r\nline-4")
    );
}

#[test]
fn aggregate_log_level_prefers_highest_severity_in_batch() {
    let entries = vec![
        BufferedLogEntry {
            timestamp: "2026-04-22T00:00:00Z".to_string(),
            level: "INFO".to_string(),
            module: "web.host.test".to_string(),
            message: "info".to_string(),
            line: "info".to_string(),
        },
        BufferedLogEntry {
            timestamp: "2026-04-22T00:00:01Z".to_string(),
            level: "WARN".to_string(),
            module: "web.host.test".to_string(),
            message: "warn".to_string(),
            line: "warn".to_string(),
        },
        BufferedLogEntry {
            timestamp: "2026-04-22T00:00:02Z".to_string(),
            level: "ERROR".to_string(),
            module: "web.host.test".to_string(),
            message: "error".to_string(),
            line: "error".to_string(),
        },
    ];

    assert_eq!(aggregate_log_level(entries.as_slice()), "error");
}

#[test]
fn ob11_config_route_returns_napcat_compatible_shape() {
    let server = test_server();
    let response = server.route_http_request(
        format!(
            "GET /api/OB11Config/GetConfig HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
            local_auth_header(&server)
        )
        .as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("ob11 config body should be valid json");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body["code"], 0);
    assert_eq!(
        body["data"]["network"]["httpServers"],
        serde_json::json!([])
    );
    assert_eq!(
        body["data"]["network"]["httpClients"],
        serde_json::json!([])
    );
    assert_eq!(
        body["data"]["network"]["httpSseServers"],
        serde_json::json!([])
    );
    assert_eq!(
        body["data"]["network"]["websocketServers"],
        serde_json::json!([])
    );
    assert_eq!(
        body["data"]["network"]["websocketClients"],
        serde_json::json!([])
    );
    assert_eq!(body["data"]["parseMultMsg"], true);
    assert_eq!(body["data"]["timeout"]["baseTimeout"], 10_000);
}

#[test]
fn i18n_route_returns_current_catalog_snapshot() {
    let server = test_server();
    let response = server.route_http_request(
        format!(
            "GET /api/i18n HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
            local_auth_header(&server)
        )
        .as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("i18n body should be valid json");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body["locale"], "zh-CN");
    assert_eq!(body["fallback_locale"], "zh-CN");
    assert_eq!(
        body["messages"]["command.spec.help.summary"],
        "显示当前作用域可用命令"
    );
    assert_eq!(body["messages"]["web.nav.overview"], "总览");
    assert_eq!(body["messages"]["web.runtime.status.running"], "运行中");
}

#[test]
fn static_asset_route_returns_injected_asset() {
    let response = test_server().route_http_request(
        b"GET /assets/bot.svg HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body = String::from_utf8(body).expect("body should be utf8");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(headers.contains("Content-Type: image/svg+xml; charset=utf-8\r\n"));
    assert_eq!(body, TEST_SVG);
}

#[test]
fn head_request_returns_headers_without_body() {
    let response = test_server().route_http_request(
        b"HEAD / HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);

    assert!(headers.starts_with("HTTP/1.1 307 Temporary Redirect\r\n"));
    assert!(headers.contains("Location: /webui/\r\n"));
    assert!(headers.contains("Content-Length: 0\r\n"));
    assert!(body.is_empty());
}

#[test]
fn missing_asset_returns_not_found() {
    let response = test_server().route_http_request(
        b"GET /missing HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body = String::from_utf8(body).expect("body should be utf8");

    assert!(headers.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert_eq!(body, "not found");
}

fn temp_dir_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("rsliteyukibot-web-host-{name}-{unique}"))
}

fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    for (path, contents) in entries {
        writer
            .start_file(
                path.replace('\\', "/"),
                zip::write::SimpleFileOptions::default(),
            )
            .expect("zip entry should be created");
        writer
            .write_all(contents)
            .expect("zip entry should be written");
    }
    writer
        .finish()
        .expect("zip writer should finish")
        .into_inner()
}

fn build_plugin_upload_request(filename: &str, archive: &[u8]) -> Vec<u8> {
    let boundary = "----RsLiteyukiPluginUpload";
    let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"plugin\"; filename=\"{filename}\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .into_bytes();
    body.extend_from_slice(archive);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let mut request = format!(
            "POST /api/Plugin/Import HTTP/1.1\r\nHost: localhost\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
    request.extend_from_slice(&body);
    request
}

fn build_authenticated_plugin_upload_request(
    server: &WebHostService,
    filename: &str,
    archive: &[u8],
) -> Vec<u8> {
    let boundary = "----RsLiteyukiPluginUpload";
    let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"plugin\"; filename=\"{filename}\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .into_bytes();
    body.extend_from_slice(archive);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let mut request = format!(
            "POST /api/Plugin/Import HTTP/1.1\r\nHost: localhost\r\n{}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\n\r\n",
            local_auth_header(server),
            body.len()
        )
        .into_bytes();
    request.extend_from_slice(&body);
    request
}

fn build_source_adapter_bundle_archive() -> Vec<u8> {
    let override_manifest = serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "source": {
            "kind": "astrbot",
            "path": "astrbot_plugin/hello_world",
            "metadataFiles": ["shared/plugin-doc.yaml"]
        }
    }))
    .expect("override manifest should serialize");
    build_test_zip(&[
        (
            "bundle/manifests/hello.override.json",
            override_manifest.as_bytes(),
        ),
        (
            "bundle/astrbot_plugin/hello_world/metadata.yaml",
            b"name: hello_world\ndisplay_name: Hello World\ndesc: greeting\n",
        ),
        (
            "bundle/astrbot_plugin/hello_world/main.py",
            b"class Hello: pass\n",
        ),
        (
            "bundle/astrbot_plugin/hello_world/_conf_schema.json",
            br#"{"token":{"type":"string"}}"#,
        ),
        ("bundle/shared/plugin-doc.yaml", b"title: plugin-doc\n"),
    ])
}

#[test]
fn install_local_plugin_archive_accepts_source_adapter_bundle() {
    let _env_guard = env_lock_guard();
    let home_root = temp_dir_path("source-adapter-install");
    fs::create_dir_all(home_root.as_path()).expect("temp home should be created");
    let _guards = [
        EnvVarGuard::set("USERPROFILE", home_root.as_path()),
        EnvVarGuard::set("HOME", home_root.as_path()),
    ];

    let archive = build_source_adapter_bundle_archive();

    let response = plugin_install::install_local_plugin_archive(
        build_plugin_upload_request("hello.zip", archive.as_slice()).as_slice(),
        None,
    )
    .expect("source-adapter bundle should install");

    assert_eq!(response["pluginId"], serde_json::json!("hello-world"));
    assert_eq!(response["pluginIds"], serde_json::json!(["hello-world"]));

    let plugin_root = resolve_local_plugin_dir();
    assert!(
        plugin_root
            .join("manifests")
            .join("hello.override.json")
            .is_file(),
        "override manifest should be installed under manifests/"
    );
    assert!(
        plugin_root
            .join("astrbot_plugin")
            .join("hello_world")
            .join("metadata.yaml")
            .is_file(),
        "source plugin tree should be installed under its family directory"
    );
    assert!(
        plugin_root.join("shared").join("plugin-doc.yaml").is_file(),
        "metadataFiles assets outside sourcePath should also be installed"
    );

    let discovered = discover_plugin_manifests_in_dirs([plugin_root.as_path()])
        .expect("installed source-adapter bundle should be discoverable");
    assert!(
        discovered
            .into_iter()
            .any(|manifest| manifest.descriptor.metadata.id == "hello-world"),
        "installed source-adapter bundle should produce a normalized manifest"
    );

    let _ = fs::remove_dir_all(home_root);
}

#[test]
fn plugin_import_route_accepts_source_adapter_bundle() {
    let _env_guard = env_lock_guard();
    let home_root = temp_dir_path("source-adapter-route-install");
    fs::create_dir_all(home_root.as_path()).expect("temp home should be created");
    let _guards = [
        EnvVarGuard::set("USERPROFILE", home_root.as_path()),
        EnvVarGuard::set("HOME", home_root.as_path()),
    ];
    let archive = build_source_adapter_bundle_archive();
    let server = test_server();

    let response = server.route_http_request(
        build_authenticated_plugin_upload_request(&server, "hello.zip", archive.as_slice())
            .as_slice(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("plugin import body should be valid json");

    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["pluginId"], "hello-world");
    assert_eq!(
        body["data"]["pluginIds"],
        serde_json::json!(["hello-world"])
    );

    let plugin_root = resolve_local_plugin_dir();
    assert!(
        plugin_root
            .join("manifests")
            .join("hello.override.json")
            .is_file(),
        "override manifest should be installed through the HTTP route"
    );

    let _ = fs::remove_dir_all(home_root);
}

#[test]
fn finalize_source_adapter_install_rolls_back_created_targets_on_conflict() {
    let root = temp_dir_path("source-adapter-finalize-rollback");
    let plugin_root = root.join("plugins");
    let install_root = root.join("install");
    fs::create_dir_all(install_root.join("astrbot_plugin").join("hello_world"))
        .expect("staged source dir should be created");
    fs::create_dir_all(install_root.join("shared")).expect("staged shared dir should exist");
    fs::write(
        install_root
            .join("astrbot_plugin")
            .join("hello_world")
            .join("metadata.yaml"),
        "name: hello_world\n",
    )
    .expect("staged source file should be written");
    fs::write(
        install_root.join("shared").join("plugin-doc.yaml"),
        "title: staged\n",
    )
    .expect("staged metadata file should be written");

    fs::create_dir_all(plugin_root.join("shared")).expect("plugin shared dir should exist");
    fs::write(
        plugin_root.join("shared").join("plugin-doc.yaml"),
        "title: existing\n",
    )
    .expect("existing conflicting file should be written");

    let result = plugin_install::finalize_source_adapter_install(
        plugin_root.as_path(),
        install_root.as_path(),
        &[
            PathBuf::from("astrbot_plugin/hello_world"),
            PathBuf::from("shared/plugin-doc.yaml"),
        ],
    );
    assert!(
        result.is_err(),
        "finalize should fail when a later install target already exists"
    );
    assert!(
        !plugin_root
            .join("astrbot_plugin")
            .join("hello_world")
            .exists(),
        "already-created targets should be rolled back after a later conflict"
    );
    assert_eq!(
        fs::read_to_string(plugin_root.join("shared").join("plugin-doc.yaml"))
            .expect("conflicting target should remain readable"),
        "title: existing\n"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn directory_asset_mode_serves_files_and_spa_fallback() {
    let root = temp_dir_path("dist");
    fs::create_dir_all(root.join("assets")).expect("asset dir should be created");
    fs::write(
        root.join("index.html"),
        "<!doctype html><title>Dist</title>",
    )
    .expect("index should be written");
    fs::write(root.join("assets").join("app.js"), "console.log('ok');")
        .expect("app.js should be written");

    let assets = WebHostAssets::new(WebHostAsset::text(
        "text/html; charset=utf-8",
        "<!doctype html><title>Fallback</title>",
    ))
    .with_asset_directory(root.clone());
    let server = WebHostService {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
        browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        dev_frontend: None,
        snapshot_provider: Arc::new(AppHostSnapshot::default),
        runtime_host: None,
        assets: Arc::new(assets),
        terminal_state: Arc::new(WebTerminalState::default()),
        auth: WebUiAuthManager::in_memory_for_tests(),
    };

    let js_response = server.route_http_request(
        b"GET /assets/app.js HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (js_headers, js_body) = split_response(js_response);
    let js_body = String::from_utf8(js_body).expect("js body should be utf8");
    assert!(js_headers.contains("Content-Type: text/javascript; charset=utf-8\r\n"));
    assert_eq!(js_body, "console.log('ok');");

    let prefixed_js_response = server.route_http_request(
        b"GET /webui/assets/app.js HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (prefixed_js_headers, prefixed_js_body) = split_response(prefixed_js_response);
    let prefixed_js_body =
        String::from_utf8(prefixed_js_body).expect("prefixed js body should be utf8");
    assert!(prefixed_js_headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(prefixed_js_headers.contains("Content-Type: text/javascript; charset=utf-8\r\n"));
    assert_eq!(prefixed_js_body, "console.log('ok');");

    let spa_response = server.route_http_request(
        b"GET /dashboard HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (spa_headers, spa_body) = split_response(spa_response);
    let spa_body = String::from_utf8(spa_body).expect("spa body should be utf8");
    assert!(spa_headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(spa_body, "<!doctype html><title>Dist</title>");

    let prefixed_spa_response = server.route_http_request(
        b"GET /webui/dashboard HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (prefixed_spa_headers, prefixed_spa_body) = split_response(prefixed_spa_response);
    let prefixed_spa_body =
        String::from_utf8(prefixed_spa_body).expect("prefixed spa body should be utf8");
    assert!(prefixed_spa_headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(prefixed_spa_body, "<!doctype html><title>Dist</title>");

    let _ = fs::remove_file(root.join("assets").join("app.js"));
    let _ = fs::remove_file(root.join("index.html"));
    let _ = fs::remove_dir(root.join("assets"));
    let _ = fs::remove_dir(root);
}

#[test]
fn dev_frontend_redirects_non_api_routes_when_probe_is_alive() {
    let probe_listener =
        StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe listener should bind");
    let probe_addr = probe_listener
        .local_addr()
        .expect("probe listener should expose local addr");
    let server = WebHostService {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
        browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        dev_frontend: Some(WebHostDevServer {
            probe_addr,
            public_port: 1420,
        }),
        snapshot_provider: Arc::new(AppHostSnapshot::default),
        runtime_host: None,
        assets: Arc::new(test_assets()),
        terminal_state: Arc::new(WebTerminalState::default()),
        auth: WebUiAuthManager::in_memory_for_tests(),
    };

    let response = server.route_http_request(
        b"GET /dashboard?tab=runtime HTTP/1.1\r\nHost: 192.168.2.2:14500\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (headers, body) = split_response(response);

    assert!(headers.starts_with("HTTP/1.1 307 Temporary Redirect\r\n"));
    assert!(headers.contains("Location: http://192.168.2.2:1420/dashboard?tab=runtime\r\n"));
    assert_eq!(
        String::from_utf8(body).expect("body should be utf8"),
        "redirecting"
    );
}

#[test]
fn dev_frontend_redirect_keeps_local_api_and_static_routes() {
    let probe_listener =
        StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe listener should bind");
    let probe_addr = probe_listener
        .local_addr()
        .expect("probe listener should expose local addr");
    let server = WebHostService {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
        browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        dev_frontend: Some(WebHostDevServer {
            probe_addr,
            public_port: 1420,
        }),
        snapshot_provider: Arc::new(AppHostSnapshot::default),
        runtime_host: None,
        assets: Arc::new(test_assets()),
        terminal_state: Arc::new(WebTerminalState::default()),
        auth: WebUiAuthManager::in_memory_for_tests(),
    };

    let api_response = server.route_http_request(
        b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (api_headers, _) = split_response(api_response);
    assert!(api_headers.starts_with("HTTP/1.1 200 OK\r\n"));

    let i18n_response = server.route_http_request(
        format!(
            "GET /api/i18n HTTP/1.1\r\nHost: localhost\r\n{}\r\n\r\n",
            local_auth_header(&server)
        )
        .as_bytes(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (i18n_headers, _) = split_response(i18n_response);
    assert!(i18n_headers.starts_with("HTTP/1.1 200 OK\r\n"));

    let icon_response = server.route_http_request(
        b"GET /favicon.ico HTTP/1.1\r\nHost: localhost\r\n\r\n",
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    );
    let (icon_headers, _) = split_response(icon_response);
    assert!(icon_headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(icon_headers.contains("Content-Type: image/x-icon\r\n"));
}
