use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::protocol::{ChatHistory, ResponsesTurnInput};
use super::{LlmFunctionTool, OpenAiResponsesClient};

pub(super) fn encode_responses_tool(tool: &LlmFunctionTool) -> Value {
    let mut value = Map::new();
    value.insert("type".to_string(), Value::String("function".to_string()));
    value.insert("name".to_string(), Value::String(tool.name.clone()));
    value.insert(
        "parameters".to_string(),
        normalize_openai_tool_parameters(&tool.parameters, tool.strict),
    );
    value.insert("strict".to_string(), Value::Bool(tool.strict));
    if let Some(description) = tool.description.as_ref() {
        value.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    Value::Object(value)
}

pub(super) fn encode_chat_tool(tool: &LlmFunctionTool) -> Value {
    let mut function = Map::new();
    function.insert("name".to_string(), Value::String(tool.name.clone()));
    function.insert(
        "parameters".to_string(),
        normalize_openai_tool_parameters(&tool.parameters, tool.strict),
    );
    function.insert("strict".to_string(), Value::Bool(tool.strict));
    if let Some(description) = tool.description.as_ref() {
        function.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    json!({
        "type": "function",
        "function": Value::Object(function),
    })
}

pub(super) fn build_responses_request(
    client: &OpenAiResponsesClient,
    input: &ResponsesTurnInput,
    tools: &[LlmFunctionTool],
    stream: bool,
) -> Value {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(client.model.clone()));

    if let Some(instructions) = client
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "instructions".to_string(),
            Value::String(instructions.to_string()),
        );
    }

    match input {
        ResponsesTurnInput::Initial { input } => {
            body.insert("input".to_string(), input.clone());
        }
        ResponsesTurnInput::ToolOutputs {
            previous_response_id,
            outputs,
        } => {
            body.insert(
                "previous_response_id".to_string(),
                Value::String(previous_response_id.clone()),
            );
            body.insert(
                "input".to_string(),
                Value::Array(outputs.iter().map(|output| output.to_value()).collect()),
            );
        }
    }

    apply_common_generation_fields(client, &mut body, tools, stream, true);
    Value::Object(body)
}

pub(super) fn build_chat_request(
    client: &OpenAiResponsesClient,
    history: &ChatHistory,
    tools: &[LlmFunctionTool],
    stream: bool,
) -> Value {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(client.model.clone()));
    body.insert(
        "messages".to_string(),
        Value::Array(history.to_value_array()),
    );

    apply_common_generation_fields(client, &mut body, tools, stream, false);
    Value::Object(body)
}

pub(super) fn supports_compat_top_k(base_url: &str) -> bool {
    !base_url
        .trim()
        .to_ascii_lowercase()
        .contains("api.openai.com")
}

pub(super) fn normalize_openai_tool_parameters(parameters: &Value, strict: bool) -> Value {
    let mut normalized = normalize_openai_schema_shape(parameters);
    if strict {
        enforce_strict_openai_schema(&mut normalized);
    }
    normalized
}

fn normalize_openai_schema_shape(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut normalized = Map::new();
            for (key, value) in map {
                normalized.insert(key.clone(), normalize_openai_schema_shape(value));
            }
            if normalized.get("type").and_then(Value::as_str) == Some("array")
                && !normalized.contains_key("items")
            {
                normalized.insert("items".to_string(), json!({ "type": "string" }));
            }
            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(normalize_openai_schema_shape)
                .collect::<Vec<_>>(),
        ),
        _ => schema.clone(),
    }
}

fn enforce_strict_openai_schema(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };

    if let Some(any_of) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
        for branch in any_of {
            enforce_strict_openai_schema(branch);
        }
        return;
    }

    if let Some(items) = object.get_mut("items") {
        enforce_strict_openai_schema(items);
    }

    if object.get("type").and_then(Value::as_str) != Some("object") {
        return;
    }

    object
        .entry("additionalProperties".to_string())
        .or_insert_with(|| Value::Bool(false));

    let mut property_keys = Vec::new();
    if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
        property_keys = properties.keys().cloned().collect::<Vec<_>>();
        for key in &property_keys {
            if let Some(property) = properties.get_mut(key) {
                make_schema_nullable(property);
                enforce_strict_openai_schema(property);
            }
        }
    }

    if property_keys.is_empty() {
        object
            .entry("required".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        return;
    }

    let existing_required = object
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    let required = property_keys
        .iter()
        .map(|key| Value::String(key.clone()))
        .collect::<Vec<_>>();
    object.insert("required".to_string(), Value::Array(required));

    if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
        for key in &property_keys {
            if existing_required.contains(key) {
                continue;
            }
            if let Some(property) = properties.get_mut(key) {
                make_schema_nullable(property);
            }
        }
    }
}

fn make_schema_nullable(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };

    if let Some(any_of) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
        if any_of
            .iter()
            .any(|branch| branch.get("type").and_then(Value::as_str) == Some("null"))
        {
            return;
        }
        any_of.push(json!({ "type": "null" }));
        return;
    }

    match object.get_mut("type") {
        Some(Value::String(kind)) if kind != "null" => {
            let original = kind.clone();
            object.insert(
                "type".to_string(),
                Value::Array(vec![
                    Value::String(original),
                    Value::String("null".to_string()),
                ]),
            );
        }
        Some(Value::Array(items)) => {
            if !items.iter().any(|item| item.as_str() == Some("null")) {
                items.push(Value::String("null".to_string()));
            }
        }
        _ => {}
    }
}

pub(super) fn llm_endpoint(base_url: &str, path_suffix: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.to_ascii_lowercase().ends_with("/v1") {
        format!("{base}/{path_suffix}")
    } else {
        format!("{base}/v1/{path_suffix}")
    }
}

fn apply_common_generation_fields(
    client: &OpenAiResponsesClient,
    body: &mut Map<String, Value>,
    tools: &[LlmFunctionTool],
    stream: bool,
    use_responses_tool_shape: bool,
) {
    if stream {
        body.insert("stream".to_string(), Value::Bool(true));
        if use_responses_tool_shape {
            body.insert(
                "stream_options".to_string(),
                json!({ "include_obfuscation": false }),
            );
        }
    }
    if let Some(temperature) = client.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = client.top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(top_k) = client
        .top_k
        .filter(|_| supports_compat_top_k(client.base_url.as_str()))
    {
        body.insert("top_k".to_string(), json!(top_k));
    }
    if let Some(frequency_penalty) = client.frequency_penalty {
        body.insert("frequency_penalty".to_string(), json!(frequency_penalty));
    }
    if let Some(presence_penalty) = client.presence_penalty {
        body.insert("presence_penalty".to_string(), json!(presence_penalty));
    }
    if let Some(reasoning_effort) = client.reasoning_effort.as_deref() {
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": reasoning_effort }),
        );
    }
    if !tools.is_empty() {
        body.insert(
            "parallel_tool_calls".to_string(),
            Value::Bool(client.parallel_tool_calls),
        );
        body.insert(
            "tools".to_string(),
            Value::Array(
                tools
                    .iter()
                    .map(|tool| {
                        if use_responses_tool_shape {
                            encode_responses_tool(tool)
                        } else {
                            encode_chat_tool(tool)
                        }
                    })
                    .collect(),
            ),
        );
    }
}
