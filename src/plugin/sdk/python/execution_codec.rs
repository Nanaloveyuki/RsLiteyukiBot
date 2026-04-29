use pyo3::types::{PyAny, PyAnyMethods};
use serde_json::Value;

use crate::plugin::PluginToolResult;
use crate::plugin::sdk::python::json_codec::py_any_to_json;
use crate::plugin::sdk::{PluginSdkError, PluginWebApiRequest, PluginWebApiResponse};

pub(super) fn normalize_tool_arguments(arguments: &Value) -> Result<Value, PluginSdkError> {
    match arguments {
        Value::Null => Ok(Value::Object(serde_json::Map::new())),
        Value::Object(_) => Ok(arguments.clone()),
        _ => Err(PluginSdkError::Runtime(
            "plugin tool arguments must be a JSON object".to_string(),
        )),
    }
}

pub(super) fn normalize_plugin_tool_lookup(plugin_id: &str, tool_name: &str) -> String {
    let trimmed = tool_name.trim();
    let prefix = format!("plugin::{plugin_id}::");
    trimmed
        .strip_prefix(prefix.as_str())
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

pub(super) fn parse_python_tool_result(
    value: &pyo3::Bound<'_, PyAny>,
) -> Result<PluginToolResult, String> {
    if value.is_none() {
        return Ok(PluginToolResult::Json(Value::Null));
    }
    if let Ok(text) = value.extract::<String>() {
        return Ok(PluginToolResult::Text(text));
    }
    let json = py_any_to_json(value).map_err(|err| err.to_string())?;
    Ok(PluginToolResult::Json(json))
}

pub(super) fn build_plugin_web_api_request_context(request: &PluginWebApiRequest) -> Value {
    let body = request.body.clone();
    let body_json = serde_json::from_slice::<Value>(body.as_slice()).ok();
    let body_text = String::from_utf8(body.clone()).ok();
    let body_bytes_base64 = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(body)
    };
    serde_json::json!({
        "method": request.method,
        "path": request.path,
        "query": request.query,
        "headers": request.headers,
        "bodyJson": body_json,
        "bodyText": body_text,
        "bodyBytesBase64": body_bytes_base64,
        "peerIp": request.peer_ip,
    })
}

pub(super) fn normalize_lookup_web_api_route(route: &str) -> String {
    let trimmed = route.trim().trim_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}")
    }
}

pub(super) fn parse_python_web_api_response(
    value: &pyo3::Bound<'_, PyAny>,
) -> Result<PluginWebApiResponse, String> {
    if value.is_none() {
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: Vec::new(),
        });
    }

    if let Ok(bytes) = value.extract::<Vec<u8>>() {
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "application/octet-stream".to_string(),
            body: bytes,
        });
    }

    if let Ok(text) = value.extract::<String>() {
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: text.into_bytes(),
        });
    }

    let json = py_any_to_json(value).map_err(|err| err.to_string())?;
    decode_json_web_api_response(json)
}

pub(super) fn decode_json_web_api_response(value: Value) -> Result<PluginWebApiResponse, String> {
    match value {
        Value::Array(items) => decode_array_web_api_response(items),
        Value::Object(mut object) => {
            let status_code = object
                .remove("status")
                .and_then(|value| value.as_u64())
                .map(|value| value as u16)
                .unwrap_or(200);
            let explicit_content_type = object
                .remove("contentType")
                .or_else(|| object.remove("content_type"))
                .and_then(|value| value.as_str().map(ToString::to_string));
            let body_value = object.remove("body").unwrap_or(Value::Object(object));
            let (content_type, body) =
                encode_web_api_body(body_value, explicit_content_type.as_deref())?;
            Ok(PluginWebApiResponse {
                status_code,
                content_type,
                body,
            })
        }
        other => {
            let (content_type, body) = encode_web_api_body(other, None)?;
            Ok(PluginWebApiResponse {
                status_code: 200,
                content_type,
                body,
            })
        }
    }
}

fn decode_array_web_api_response(items: Vec<Value>) -> Result<PluginWebApiResponse, String> {
    let Some(first) = items.first() else {
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "application/json; charset=utf-8".to_string(),
            body: b"[]".to_vec(),
        });
    };
    let Some(status_code) = first.as_u64().map(|value| value as u16) else {
        let body = serde_json::to_vec(&Value::Array(items))
            .map_err(|err| format!("response json serialization failed: {err}"))?;
        return Ok(PluginWebApiResponse {
            status_code: 200,
            content_type: "application/json; charset=utf-8".to_string(),
            body,
        });
    };
    let body_value = items.get(1).cloned().unwrap_or(Value::Null);
    let content_type = items
        .get(2)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let (content_type, body) = encode_web_api_body(body_value, content_type)?;
    Ok(PluginWebApiResponse {
        status_code,
        content_type,
        body,
    })
}

fn encode_web_api_body(
    value: Value,
    explicit_content_type: Option<&str>,
) -> Result<(String, Vec<u8>), String> {
    match value {
        Value::Null => Ok((
            explicit_content_type
                .unwrap_or("text/plain; charset=utf-8")
                .to_string(),
            Vec::new(),
        )),
        Value::String(text) => Ok((
            explicit_content_type
                .unwrap_or("text/plain; charset=utf-8")
                .to_string(),
            text.into_bytes(),
        )),
        other => Ok((
            explicit_content_type
                .unwrap_or("application/json; charset=utf-8")
                .to_string(),
            serde_json::to_vec(&other)
                .map_err(|err| format!("response json serialization failed: {err}"))?,
        )),
    }
}

#[cfg(test)]
#[path = "execution_codec/tests.rs"]
mod tests;
