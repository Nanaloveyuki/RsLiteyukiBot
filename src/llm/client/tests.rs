use super::event_dispatch::EventDispatcher;
use super::protocol::ResponsesTurnInput;
use super::*;
use liteyukibot_core::observability::set_console_log_output_enabled;
use liteyukibot_core::recent_buffered_logs;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};
use serde_json::json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

fn spawn_mock_http_server(raw_responses: Vec<String>) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let addr = listener.local_addr().expect("listener addr should exist");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured_requests = Arc::clone(&requests);

    thread::spawn(move || {
        for raw_response in raw_responses {
            let (mut socket, _) = listener.accept().expect("connection should be accepted");
            let mut request = Vec::new();
            let mut header_buf = [0_u8; 4096];
            loop {
                let read = socket
                    .read(&mut header_buf)
                    .expect("request should be readable");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&header_buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4)
                .expect("request should have header separator");
            let header_text = String::from_utf8_lossy(&request[..header_end]);
            let content_length = header_text
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let read = socket
                    .read(&mut header_buf)
                    .expect("body should be readable");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&header_buf[..read]);
            }
            captured_requests
                .lock()
                .expect("request capture lock should be available")
                .push(String::from_utf8_lossy(&request).to_string());

            socket
                .write_all(raw_response.as_bytes())
                .expect("response should be writable");
        }
    });

    (addr, requests)
}

fn http_json_response(body: Value) -> String {
    let body = body.to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn http_sse_response(events: &[Value]) -> String {
    let body = events
        .iter()
        .map(|event| format!("data: {}\n\n", event))
        .collect::<String>();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn request_body_json(raw_request: &str) -> Value {
    let body = raw_request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("request should contain header separator");
    serde_json::from_str(body).expect("request body should be valid json")
}

#[derive(Clone)]
struct TestConfig {
    base_url: String,
    stream: bool,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    frequency_penalty: Option<f32>,
    presence_penalty: Option<f32>,
}

impl OpenAiRuntimeConfig for TestConfig {
    fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    fn model(&self) -> &str {
        "gpt-5.2"
    }

    fn timeout_ms(&self) -> u64 {
        2_000
    }

    fn system_prompt(&self) -> Option<&str> {
        Some("system prompt")
    }

    fn stream(&self) -> bool {
        self.stream
    }

    fn temperature(&self) -> Option<f32> {
        self.temperature
    }

    fn top_p(&self) -> Option<f32> {
        self.top_p
    }

    fn top_k(&self) -> Option<u32> {
        self.top_k
    }

    fn frequency_penalty(&self) -> Option<f32> {
        self.frequency_penalty
    }

    fn presence_penalty(&self) -> Option<f32> {
        self.presence_penalty
    }
}

#[tokio::test(flavor = "current_thread")]
async fn responses_stream_emits_text_deltas_and_final_text() {
    let events = vec![
        json!({
            "type": "response.output_text.delta",
            "delta": "hel",
        }),
        json!({
            "type": "response.output_text.delta",
            "delta": "lo",
        }),
        json!({
            "type": "response.completed",
            "response": {
                "id": "resp_1",
                "output_text": "hello"
            }
        }),
    ];
    let (addr, requests) = spawn_mock_http_server(vec![http_sse_response(&events)]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: true,
        temperature: Some(0.3),
        top_p: Some(0.9),
        top_k: Some(40),
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");

    let mut seen = Vec::new();
    let completion = client
        .generate_with_events("hello", &mut |event| seen.push(event))
        .await
        .expect("streaming call should succeed");

    assert_eq!(completion.text, "hello");
    assert_eq!(
        seen,
        vec![
            LlmStreamEvent::TextDelta("hel".to_string()),
            LlmStreamEvent::TextDelta("lo".to_string()),
        ]
    );

    let captured = requests.lock().expect("request capture should lock");
    assert_eq!(captured.len(), 1);
    let body = request_body_json(&captured[0]);
    assert_eq!(body.get("stream").and_then(Value::as_bool), Some(true));
    assert_eq!(
        body.get("instructions").and_then(Value::as_str),
        Some("system prompt")
    );
    assert!(
        body.get("temperature")
            .and_then(Value::as_f64)
            .is_some_and(|value| (value - 0.3_f64).abs() < 1e-6)
    );
    assert!(
        body.get("top_p")
            .and_then(Value::as_f64)
            .is_some_and(|value| (value - 0.9_f64).abs() < 1e-6)
    );
    assert_eq!(body.get("top_k").and_then(Value::as_u64), Some(40));
}

#[tokio::test(flavor = "current_thread")]
async fn tool_call_loop_submits_function_call_output() {
    let initial_response = json!({
        "id": "resp_1",
        "output": [
            {
                "id": "item_1",
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup_weather",
                "arguments": "{\"city\":\"Paris\"}"
            }
        ]
    });
    let final_response = json!({
        "id": "resp_2",
        "output_text": "Sunny in Paris"
    });
    let (addr, requests) = spawn_mock_http_server(vec![
        http_json_response(initial_response),
        http_json_response(final_response),
    ]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");
    let tool = LlmFunctionTool::new(
        "lookup_weather",
        json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        }),
        |arguments| async move {
            assert_eq!(arguments["city"], "Paris");
            Ok(LlmToolOutput::Text("Sunny in Paris".to_string()))
        },
    )
    .with_description("Look up the weather");

    let completion = client
        .complete("weather", &[tool], None)
        .await
        .expect("tool loop should succeed");

    assert_eq!(completion.text, "Sunny in Paris");
    assert_eq!(completion.tool_calls.len(), 1);
    assert_eq!(completion.tool_calls[0].call_id, "call_1");
    assert_eq!(completion.tool_calls[0].name, "lookup_weather");
    assert_eq!(completion.tool_calls[0].output, "Sunny in Paris");

    let captured = requests.lock().expect("request capture should lock");
    assert_eq!(captured.len(), 2);
    assert!(captured[0].contains("\"tools\":["));
    assert!(captured[1].contains("\"previous_response_id\":\"resp_1\""));
    assert!(captured[1].contains("\"type\":\"function_call_output\""));
    assert!(captured[1].contains("\"call_id\":\"call_1\""));
    assert!(captured[1].contains("\"output\":\"Sunny in Paris\""));
}

#[tokio::test(flavor = "current_thread")]
async fn complete_with_chat_completions_uses_chat_endpoint_and_returns_text() {
    let response = json!({
        "id": "chatcmpl_1",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": "hello from chat completions"
                }
            }
        ]
    });
    let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: Some(0.4),
        top_p: Some(0.85),
        top_k: Some(32),
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");

    let completion = client
        .complete_with_chat_completions("hello", &[], None)
        .await
        .expect("chat completions request should succeed");
    assert_eq!(completion.text, "hello from chat completions");
    assert!(completion.tool_calls.is_empty());

    let captured = requests.lock().expect("request capture should lock");
    assert_eq!(captured.len(), 1);
    assert!(captured[0].contains("POST /v1/chat/completions HTTP/1.1"));
    let body = request_body_json(&captured[0]);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "system prompt");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][1]["content"], "hello");
    assert!(
        body.get("temperature")
            .and_then(Value::as_f64)
            .is_some_and(|value| (value - 0.4_f64).abs() < 1e-6)
    );
    assert!(
        body.get("top_p")
            .and_then(Value::as_f64)
            .is_some_and(|value| (value - 0.85_f64).abs() < 1e-6)
    );
    assert_eq!(body.get("top_k").and_then(Value::as_u64), Some(32));
}

#[tokio::test(flavor = "current_thread")]
async fn complete_with_chat_messages_submits_tool_outputs() {
    let initial_response = json!({
        "id": "chatcmpl_1",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "lookup_weather",
                                "arguments": "{\"city\":\"Paris\"}"
                            }
                        }
                    ]
                }
            }
        ]
    });
    let final_response = json!({
        "id": "chatcmpl_2",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": "Sunny in Paris"
                }
            }
        ]
    });
    let (addr, requests) = spawn_mock_http_server(vec![
        http_json_response(initial_response),
        http_json_response(final_response),
    ]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");
    let tool = LlmFunctionTool::new(
        "lookup_weather",
        json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        }),
        |arguments| async move {
            assert_eq!(arguments["city"], "Paris");
            Ok(LlmToolOutput::Text("Sunny in Paris".to_string()))
        },
    )
    .with_description("Look up the weather");

    let completion = client
        .complete_with_chat_messages(
            vec![json!({
                "role": "user",
                "content": "weather"
            })],
            &[tool],
            None,
        )
        .await
        .expect("chat tool loop should succeed");

    assert_eq!(completion.text, "Sunny in Paris");
    assert_eq!(completion.tool_calls.len(), 1);
    assert_eq!(completion.tool_calls[0].call_id, "call_1");

    let captured = requests.lock().expect("request capture should lock");
    assert_eq!(captured.len(), 2);
    assert!(captured[0].contains("POST /v1/chat/completions HTTP/1.1"));
    assert!(captured[1].contains("POST /v1/chat/completions HTTP/1.1"));
    let first_body = request_body_json(&captured[0]);
    let second_body = request_body_json(&captured[1]);
    assert_eq!(first_body["messages"][0]["role"], "user");
    assert_eq!(first_body["messages"][0]["content"], "weather");
    assert_eq!(second_body["messages"][1]["role"], "assistant");
    assert_eq!(second_body["messages"][1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(second_body["messages"][2]["role"], "tool");
    assert_eq!(second_body["messages"][2]["tool_call_id"], "call_1");
    assert_eq!(second_body["messages"][2]["content"], "Sunny in Paris");
}

#[tokio::test(flavor = "current_thread")]
async fn openai_requests_omit_compat_top_k() {
    let response = json!({
        "id": "resp_1",
        "output_text": "hello"
    });
    let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
    let config = TestConfig {
        base_url: format!("http://{addr}/proxy/api.openai.com"),
        stream: false,
        temperature: Some(0.2),
        top_p: Some(0.8),
        top_k: Some(64),
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");

    let output = client
        .generate("hello")
        .await
        .expect("request should succeed");
    assert_eq!(output, "hello");

    let captured = requests.lock().expect("request capture should lock");
    assert_eq!(captured.len(), 1);
    let body = request_body_json(&captured[0]);
    assert!(body.get("top_k").is_none());
    assert!(
        body.get("temperature")
            .and_then(Value::as_f64)
            .is_some_and(|value| (value - 0.2_f64).abs() < 1e-6)
    );
    assert!(
        body.get("top_p")
            .and_then(Value::as_f64)
            .is_some_and(|value| (value - 0.8_f64).abs() < 1e-6)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn request_debug_log_includes_json_body_before_send() {
    let _lock = process_state_lock();
    let _level_guard = EnvVarGuard::set("LY_LOG_LEVEL", "debug");
    let previous_console = set_console_log_output_enabled(false);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let prompt = format!("hello-debug-{unique}");
    let response = json!({
        "id": "resp_1",
        "output_text": "ok"
    });
    let (addr, _) = spawn_mock_http_server(vec![http_json_response(response)]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: Some(0.2),
        top_p: Some(0.8),
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");

    let output = client
        .generate(prompt.as_str())
        .await
        .expect("request should work");
    assert_eq!(output, "ok");

    assert!(recent_buffered_logs(200).iter().any(|entry| {
        entry.module == "llm.request"
            && entry.level == "DEBUG"
            && entry.message.contains("/responses")
            && entry.message.contains(prompt.as_str())
    }));

    set_console_log_output_enabled(previous_console);
}

#[test]
fn extract_output_text_prefers_direct_output_text() {
    let payload = serde_json::json!({
        "output_text": "hello world"
    });
    assert_eq!(
        extract_output_text(&payload).as_deref(),
        Some("hello world")
    );
}

#[test]
fn extract_output_text_reads_responses_output_content() {
    let payload = serde_json::json!({
        "output": [
            {
                "content": [
                    { "type": "output_text", "text": "first" },
                    { "type": "output_text", "text": "second" }
                ]
            }
        ]
    });
    assert_eq!(
        extract_output_text(&payload).as_deref(),
        Some("first\nsecond")
    );
}

#[test]
fn extract_output_text_falls_back_to_chat_completions_shape() {
    let payload = serde_json::json!({
        "choices": [
            { "message": { "content": "fallback text" } }
        ]
    });
    assert_eq!(
        extract_output_text(&payload).as_deref(),
        Some("fallback text")
    );
}

#[test]
fn extract_output_text_reads_structured_chat_content_parts() {
    let payload = serde_json::json!({
        "choices": [
            {
                "message": {
                    "content": [
                        { "type": "text", "text": "first" },
                        { "type": "text", "text": "second" }
                    ]
                }
            }
        ]
    });
    assert_eq!(
        extract_output_text(&payload).as_deref(),
        Some("first\nsecond")
    );
}

#[test]
fn llm_endpoint_adds_v1_when_missing() {
    assert_eq!(
        llm_endpoint("https://api.openai.com", "responses"),
        "https://api.openai.com/v1/responses"
    );
}

#[test]
fn llm_endpoint_keeps_existing_v1() {
    assert_eq!(
        llm_endpoint("https://tokenflux.dev/v1", "responses"),
        "https://tokenflux.dev/v1/responses"
    );
}

#[test]
fn fallback_triggered_by_instruction_required_error() {
    assert!(should_fallback_to_chat_completions(
        400,
        r#"{"detail":"Instructions are required"}"#
    ));
}

#[test]
fn chat_stream_chunk_ignores_non_choice_metadata_frames() {
    let payload = serde_json::json!({
        "id": "chatcmpl_meta",
        "object": "chat.completion.chunk"
    });
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut dispatcher = EventDispatcher::new(None);

    apply_chat_stream_chunk(&payload, &mut text, &mut tool_calls, &mut dispatcher)
        .expect("metadata-only frame should be ignored");

    assert!(text.is_empty());
    assert!(tool_calls.is_empty());
}

#[test]
fn chat_stream_chunk_merges_indexless_tool_call_deltas_into_first_slot() {
    let first = serde_json::json!({
        "choices": [{
            "delta": {
                "tool_calls": [{
                    "id": "call_1",
                    "function": {
                        "name": "demo_tool",
                        "arguments": "{\"a\":"
                    }
                }]
            }
        }]
    });
    let second = serde_json::json!({
        "choices": [{
            "delta": {
                "tool_calls": [{
                    "function": {
                        "arguments": "1}"
                    }
                }]
            }
        }]
    });
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut dispatcher = EventDispatcher::new(None);

    apply_chat_stream_chunk(&first, &mut text, &mut tool_calls, &mut dispatcher)
        .expect("first chunk should parse");
    apply_chat_stream_chunk(&second, &mut text, &mut tool_calls, &mut dispatcher)
        .expect("second chunk should merge into first tool call");

    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].call_id, "call_1");
    assert_eq!(tool_calls[0].name, "demo_tool");
    assert_eq!(tool_calls[0].arguments, "{\"a\":1}");
}

#[tokio::test(flavor = "current_thread")]
async fn strict_tool_schema_marks_optional_fields_nullable_and_required() {
    let response = json!({
        "id": "chatcmpl_1",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": "ok"
                }
            }
        ]
    });
    let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");
    let tool = LlmFunctionTool::new(
        "workspace_list_files",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "max_depth": { "type": "integer" }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        |_arguments| async move { Ok(LlmToolOutput::Text("[]".to_string())) },
    );

    let completion = client
        .complete_with_chat_completions("hello", &[tool], None)
        .await
        .expect("chat completions request should succeed");
    assert_eq!(completion.text, "ok");

    let captured = requests.lock().expect("request capture should lock");
    let body = request_body_json(&captured[0]);
    assert!(
        body["tools"][0]["function"].get("description").is_none(),
        "chat tool without description should omit the field"
    );
    let parameters = &body["tools"][0]["function"]["parameters"];
    let mut required = parameters["required"]
        .as_array()
        .expect("required should be an array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    required.sort_unstable();
    assert_eq!(required, vec!["max_depth", "path"]);
    assert_eq!(parameters["additionalProperties"], Value::Bool(false));
    assert_eq!(
        parameters["properties"]["path"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(
        parameters["properties"]["max_depth"]["type"],
        json!(["integer", "null"])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn tool_requests_omit_description_when_tool_has_no_description() {
    let response = json!({
        "id": "resp_1",
        "output_text": "ok"
    });
    let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");
    let tool = LlmFunctionTool::new(
        "workspace_list_files",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        }),
        |_arguments| async move { Ok(LlmToolOutput::Text("[]".to_string())) },
    );

    client
        .complete("hello", &[tool], None)
        .await
        .expect("responses request should succeed");

    let captured = requests.lock().expect("request capture should lock");
    let body = request_body_json(&captured[0]);
    assert_eq!(body["tools"][0]["name"], "workspace_list_files");
    assert!(
        body["tools"][0].get("description").is_none(),
        "responses tool without description should omit the field"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn custom_openai_endpoint_keeps_reasoning_when_tools_are_present() {
    let response = json!({
        "id": "chatcmpl_1",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": "ok"
                }
            }
        ]
    });
    let (addr, requests) = spawn_mock_http_server(vec![http_json_response(response)]);
    let config = TestConfig {
        base_url: format!("http://{addr}/proxy/openai"),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let mut client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");
    client.reasoning_effort = Some("medium".to_string());
    let tool = LlmFunctionTool::new(
        "lookup_weather",
        json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        }),
        |_arguments| async move { Ok(LlmToolOutput::Text("ok".to_string())) },
    );

    client
        .complete_with_chat_completions("hello", &[tool], None)
        .await
        .expect("chat request should succeed");

    let captured = requests.lock().expect("request capture should lock");
    let body = request_body_json(&captured[0]);
    assert_eq!(body["reasoning"]["effort"], "medium");
}

#[tokio::test(flavor = "current_thread")]
async fn success_status_json_error_is_reported_as_upstream_failure() {
    let response = json!({
        "error": {
            "message": "Copilot API error: Bad Request",
            "code": 400
        }
    });
    let (addr, _) = spawn_mock_http_server(vec![http_json_response(response)]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");

    let err = client
        .complete_with_chat_completions("hello", &[], None)
        .await
        .expect_err("embedded error should fail the request");
    match err {
        LlmClientError::Upstream { status, detail } => {
            assert_eq!(status, 400);
            assert!(detail.contains("Bad Request"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn embedded_instruction_error_without_numeric_code_falls_back_to_chat() {
    let responses_error = json!({
        "error": {
            "message": "Instructions are required"
        }
    });
    let chat_response = json!({
        "id": "chatcmpl_1",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": "fallback ok"
                }
            }
        ]
    });
    let (addr, requests) = spawn_mock_http_server(vec![
        http_json_response(responses_error),
        http_json_response(chat_response),
    ]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");

    let completion = client
        .complete("hello", &[], None)
        .await
        .expect("instruction error should fall back to chat");
    assert_eq!(completion.text, "fallback ok");

    let captured = requests.lock().expect("request capture should lock");
    assert_eq!(captured.len(), 2);
    assert!(captured[0].contains("POST /v1/responses HTTP/1.1"));
    assert!(captured[1].contains("POST /v1/chat/completions HTTP/1.1"));
}

#[tokio::test(flavor = "current_thread")]
async fn responses_stream_instruction_error_is_classified_as_bad_request() {
    let events = vec![json!({
        "type": "response.failed",
        "response": {
            "error": {
                "message": "Instructions are required"
            }
        }
    })];
    let (addr, _) = spawn_mock_http_server(vec![http_sse_response(&events)]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: true,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");
    let request = build_responses_request(
        &client,
        &ResponsesTurnInput::Initial {
            input: Value::String("hello".to_string()),
        },
        &[],
        true,
    );
    let mut dispatcher = EventDispatcher::new(None);

    let err = client
        .send_responses_stream(
            llm_endpoint(client.base_url.as_str(), "responses").as_str(),
            request,
            &mut dispatcher,
        )
        .await
        .expect_err("response.failed event should fail the stream");
    match err {
        LlmClientError::Upstream { status, detail } => {
            assert_eq!(status, 400);
            assert!(detail.contains("Instructions are required"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn chat_stream_reports_embedded_sse_errors() {
    let events = vec![json!({
        "error": {
            "message": "Copilot API error: Bad Request",
            "code": 400
        }
    })];
    let (addr, _) = spawn_mock_http_server(vec![http_sse_response(&events)]);
    let config = TestConfig {
        base_url: format!("http://{addr}"),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        frequency_penalty: None,
        presence_penalty: None,
    };
    let client = OpenAiResponsesClient::from_runtime_with_api_key(&config, "sk-test")
        .expect("client should build");
    let mut seen = Vec::new();

    let err = client
        .complete_with_chat_completions("hello", &[], Some(&mut |event| seen.push(event)))
        .await
        .expect_err("embedded SSE error should fail the request");
    assert!(seen.is_empty());
    match err {
        LlmClientError::Upstream { status, detail } => {
            assert_eq!(status, 400);
            assert!(detail.contains("Bad Request"));
        }
        other => panic!("unexpected error: {other}"),
    }
}
