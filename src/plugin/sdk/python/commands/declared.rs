use serde_json::Value;

use crate::plugin::PluginCommandDescriptor;

use super::super::state::{PythonDeclaredCommandEntry, PythonRuntimeState};
use super::normalization::{
    normalize_plugin_scopes, normalize_tui_command_name, plugin_scope_matches,
};
use super::scope_state::is_scope_command_disabled;

pub(crate) fn disabled_declared_command_for_plugin(
    state: &PythonRuntimeState,
    plugin_id: &str,
    payload: &Value,
) -> Option<String> {
    let scope = adapter_scope_for_payload(payload)?;
    let message = extract_payload_message_text(payload)?;
    state
        .declared_commands
        .iter()
        .find(|entry| {
            entry.plugin_id == plugin_id
                && is_scope_command_disabled(state, scope, entry.command.as_str())
                && plugin_scope_matches(&entry.scopes, scope)
                && declared_command_matches_message(entry.command.as_str(), message.as_str())
        })
        .map(|entry| entry.command.clone())
}

pub(crate) fn register_declared_commands(
    commands: &mut Vec<PythonDeclaredCommandEntry>,
    plugin_id: &str,
    descriptors: &[PluginCommandDescriptor],
) {
    commands.retain(|command| command.plugin_id.as_str() != plugin_id);
    commands.extend(descriptors.iter().filter_map(|descriptor| {
        let command = normalize_tui_command_name(descriptor.name.as_str())?;
        let description = descriptor.description.trim();
        Some(PythonDeclaredCommandEntry {
            command,
            description: if description.is_empty() {
                "plugin declared command".to_string()
            } else {
                description.to_string()
            },
            plugin_id: plugin_id.to_string(),
            scopes: normalize_plugin_scopes(&descriptor.scopes),
        })
    }));
}

fn adapter_scope_for_payload(payload: &Value) -> Option<&'static str> {
    let object = payload.as_object()?;
    let protocol = object
        .get("_adapter_protocol")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let is_onebot_v11 = protocol.eq_ignore_ascii_case("onebot.v11")
        || object.contains_key("post_type")
        || object.contains_key("meta_event_type");
    if !is_onebot_v11 || object.get("post_type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    Some("adapter:onebot11")
}

fn extract_payload_message_text(payload: &Value) -> Option<String> {
    let object = payload.as_object()?;
    for key in ["raw_message", "text"] {
        if let Some(text) = object.get(key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    if let Some(text) = object.get("message").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let segments = object.get("message")?.as_array()?;
    let mut text = String::new();
    for segment in segments {
        if let Some(raw) = segment.as_str() {
            text.push_str(raw);
            continue;
        }
        let Some(segment_object) = segment.as_object() else {
            continue;
        };
        if let Some(data_text) = segment_object
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("text"))
            .and_then(Value::as_str)
        {
            text.push_str(data_text);
            continue;
        }
        if let Some(segment_text) = segment_object.get("text").and_then(Value::as_str) {
            text.push_str(segment_text);
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn declared_command_matches_message(command: &str, message: &str) -> bool {
    let Some(command) = normalize_tui_command_name(command) else {
        return false;
    };
    let message = message.trim();
    if matches_command_with_prefix(message, command.as_str()) {
        return true;
    }
    command
        .strip_prefix('/')
        .is_some_and(|bare| matches_command_with_prefix(message, bare))
}

fn matches_command_with_prefix(message: &str, command_prefix: &str) -> bool {
    let message = message.trim();
    if message.eq_ignore_ascii_case(command_prefix) {
        return true;
    }
    parse_command_argument(message, command_prefix).is_some()
}

fn parse_command_argument(message: &str, command_prefix: &str) -> Option<String> {
    let command_prefix = command_prefix.trim();
    if command_prefix.is_empty() {
        return None;
    }

    let message = message.trim();
    if message.eq_ignore_ascii_case(command_prefix) {
        return Some(String::new());
    }

    let remainder = message.strip_prefix(command_prefix)?;
    let mut chars = remainder.chars();
    if !chars.next().is_some_and(char::is_whitespace) {
        return None;
    }

    Some(remainder.trim().to_string())
}
