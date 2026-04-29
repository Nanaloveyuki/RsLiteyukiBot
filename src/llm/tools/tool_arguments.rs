use serde_json::Value;

use crate::llm::client::LlmClientError;

pub(super) fn required_string(arguments: &Value, key: &str) -> Result<String, LlmClientError> {
    optional_string(arguments, key)?
        .ok_or_else(|| LlmClientError::Tool(format!("missing required argument '{key}'")))
}

pub(super) fn optional_string(
    arguments: &Value,
    key: &str,
) -> Result<Option<String>, LlmClientError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.trim().to_string())),
        Some(_) => Err(LlmClientError::Tool(format!(
            "argument '{key}' must be a string"
        ))),
    }
}

pub(super) fn optional_usize(
    arguments: &Value,
    key: &str,
) -> Result<Option<usize>, LlmClientError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_u64()
            .map(|value| value as usize)
            .map(Some)
            .ok_or_else(|| {
                LlmClientError::Tool(format!("argument '{key}' must be a non-negative integer"))
            }),
        Some(_) => Err(LlmClientError::Tool(format!(
            "argument '{key}' must be a non-negative integer"
        ))),
    }
}
