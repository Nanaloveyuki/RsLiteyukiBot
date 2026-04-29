use serde_json::json;

use super::{decode_json_web_api_response, normalize_plugin_tool_lookup, normalize_tool_arguments};

#[test]
// 必要测试
fn normalize_plugin_tool_lookup_strips_runtime_prefix() {
    let normalized = normalize_plugin_tool_lookup("demo", "plugin::demo::tool_a");
    assert_eq!(normalized, "tool_a");
}

#[test]
// 必要测试
fn decode_json_web_api_response_uses_explicit_content_type() {
    let response = decode_json_web_api_response(json!({
        "status": 201,
        "contentType": "text/plain",
        "body": "ok"
    }))
    .expect("response should decode");

    assert_eq!(response.status_code, 201);
    assert_eq!(response.content_type, "text/plain");
    assert_eq!(response.body, b"ok");
}

#[test]
// 必要测试
fn normalize_tool_arguments_rejects_non_object() {
    let error =
        normalize_tool_arguments(&json!(["bad"])).expect_err("non-object arguments should fail");
    assert!(error.to_string().contains("must be a JSON object"));
}
