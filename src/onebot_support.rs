use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use liteyukibot_core::core::BotEvent;
use liteyukibot_core::session::SessionEvent;
use serde_json::Value;

const EXTERNAL_HELP_TEXT: &str = "Liteyuki 外部命令:\n/help - 显示当前帮助\n\n说明:\n/log 与 /reload 属于控制台管理命令，推荐在 TUI（或未来 Tauri/Web 终端）执行。";
static OB11_LOG_IMAGE_SUMMARY: LazyLock<bool> =
    LazyLock::new(|| parse_env_bool("LY_OB11_LOG_IMAGE_SUMMARY", false));
static HELP_WHITELIST_DEBUG: LazyLock<bool> =
    LazyLock::new(|| parse_env_bool("LY_HELP_WHITELIST_DEBUG", false));

pub(crate) fn whitelist_debug_enabled() -> bool {
    *HELP_WHITELIST_DEBUG
}

pub(crate) fn is_help_command(message: &str) -> bool {
    matches!(message.trim(), "/help" | "help")
}

pub(crate) fn is_help_session_allowed(event: &SessionEvent, whitelist: &HashSet<String>) -> bool {
    whitelist.is_empty() || matched_help_whitelist_entry(event, whitelist).is_some()
}

pub(crate) fn matched_help_whitelist_entry(
    event: &SessionEvent,
    whitelist: &HashSet<String>,
) -> Option<String> {
    whitelist
        .iter()
        .find(|entry| whitelist_entry_matches(event, entry))
        .cloned()
}

pub(crate) fn whitelist_entry_matches(event: &SessionEvent, entry: &str) -> bool {
    let entry = entry.trim();
    if entry.is_empty() {
        return false;
    }

    if let Some((scope, id)) = entry.split_once(':') {
        let scope = scope.trim().to_ascii_lowercase();
        let id = id.trim();
        if id.is_empty() {
            return false;
        }

        return match scope.as_str() {
            "session" => event.session_id.as_ref() == id,
            "user" => event.user_id.as_ref() == id,
            // Keep private/group rules tolerant to scope drift, but never cross-match each other.
            "private" => {
                is_private_semantic(event)
                    && (event.user_id.as_ref() == id || event.session_id.as_ref() == id)
            }
            "group" => {
                is_group_semantic(event)
                    && (event_group_id(event).as_deref() == Some(id)
                        || event.session_id.as_ref() == id)
            }
            _ => false,
        };
    }

    event.session_id.as_ref() == entry
}

fn payload_message_type(event: &SessionEvent) -> Option<&str> {
    event.payload.get("message_type").and_then(Value::as_str)
}

fn event_group_id(event: &SessionEvent) -> Option<String> {
    event.payload.get("group_id").and_then(value_to_string)
}

fn is_group_semantic(event: &SessionEvent) -> bool {
    matches!(event.scope, liteyukibot_core::SessionScope::Group)
        || payload_message_type(event).is_some_and(|ty| ty.eq_ignore_ascii_case("group"))
        || event_group_id(event).is_some()
}

fn is_private_semantic(event: &SessionEvent) -> bool {
    if is_group_semantic(event) {
        return false;
    }
    matches!(event.scope, liteyukibot_core::SessionScope::Private)
        || payload_message_type(event).is_some_and(|ty| ty.eq_ignore_ascii_case("private"))
        || event.session_id == event.user_id
}

pub(crate) fn is_onebot_v11_payload(payload: &Value) -> bool {
    let Some(object) = payload.as_object() else {
        return false;
    };
    if object
        .get("_adapter_protocol")
        .and_then(Value::as_str)
        .is_some_and(|raw| raw.eq_ignore_ascii_case("onebot.v11"))
    {
        return true;
    }
    object.contains_key("post_type") || object.contains_key("meta_event_type")
}

pub(crate) fn build_onebot_v11_help_reply_payload(
    event: &SessionEvent,
    echo: &str,
) -> Option<Value> {
    let object = event.payload.as_object()?;
    if object.get("post_type").and_then(Value::as_str) != Some("message") {
        return None;
    }

    let message_type = object
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("private");

    let mut params = serde_json::Map::new();
    params.insert(
        "message_type".to_string(),
        Value::String(message_type.to_string()),
    );
    params.insert(
        "message".to_string(),
        Value::String(EXTERNAL_HELP_TEXT.to_string()),
    );

    if message_type.eq_ignore_ascii_case("group") {
        params.insert("group_id".to_string(), object.get("group_id")?.clone());
    } else {
        params.insert("user_id".to_string(), object.get("user_id")?.clone());
    }

    let mut payload = serde_json::Map::new();
    payload.insert("action".to_string(), Value::String("send_msg".to_string()));
    payload.insert("params".to_string(), Value::Object(params));
    payload.insert("echo".to_string(), Value::String(echo.to_string()));
    Some(Value::Object(payload))
}

pub(crate) fn value_to_string(value: &Value) -> Option<String> {
    if let Some(raw) = value.as_str() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_u64() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_i64() {
        return Some(raw.to_string());
    }
    None
}

pub(crate) fn should_hide_event_from_tui(event: &BotEvent) -> bool {
    is_onebot_v11_heartbeat(&event.payload)
}

pub(crate) fn is_onebot_v11_heartbeat(payload: &Value) -> bool {
    payload.as_object().is_some_and(|object| {
        object.get("post_type").and_then(Value::as_str) == Some("meta_event")
            && object.get("meta_event_type").and_then(Value::as_str) == Some("heartbeat")
    })
}

pub(crate) fn payload_preview(payload: &Value) -> String {
    if let Some(onebot) = payload_preview_onebot_v11(payload) {
        return truncate_preview(&onebot, 140);
    }
    truncate_preview(&payload.to_string(), 96)
}

pub(crate) fn payload_preview_onebot_v11(payload: &Value) -> Option<String> {
    let object = payload.as_object()?;
    let post_type = object.get("post_type")?.as_str()?;

    match post_type {
        "message" => Some(render_onebot_message_preview(
            object,
            *OB11_LOG_IMAGE_SUMMARY,
        )),
        "notice" => {
            let notice_type = object
                .get("notice_type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            Some(format!("ob11 notice[{notice_type}]"))
        }
        "request" => {
            let request_type = object
                .get("request_type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            Some(format!("ob11 request[{request_type}]"))
        }
        "meta_event" => {
            let meta_type = object
                .get("meta_event_type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            Some(format!("ob11 meta_event[{meta_type}]"))
        }
        other => Some(format!("ob11 {other}")),
    }
}

pub(crate) fn extract_onebot_message_preview(message: &Value) -> Option<String> {
    if let Some(raw) = message.as_str() {
        return Some(raw.to_string());
    }

    let segments = message.as_array()?;
    let mut out = String::new();
    for segment in segments {
        if let Some(text) = segment
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("text"))
            .and_then(Value::as_str)
        {
            out.push_str(text);
        }
    }
    Some(out)
}

pub(crate) fn render_onebot_message_preview(
    object: &serde_json::Map<String, Value>,
    include_image_summary: bool,
) -> String {
    let message_type = object
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let user_id = object
        .get("user_id")
        .and_then(preview_value_to_string)
        .unwrap_or_else(|| "?".to_string());

    let text = object
        .get("raw_message")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            object
                .get("message")
                .and_then(extract_onebot_message_preview)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "<empty>".to_string());
    let context = render_onebot_message_context(&text, include_image_summary);

    if message_type.eq_ignore_ascii_case("group") {
        let group_id = object
            .get("group_id")
            .and_then(preview_value_to_string)
            .unwrap_or_else(|| "?".to_string());
        return format!("群聊[{group_id}:{user_id}] {context}");
    }

    if message_type.eq_ignore_ascii_case("private") {
        return format!("私聊[{user_id}] {context}");
    }

    format!("消息[{message_type}:{user_id}] {context}")
}

pub(crate) fn render_onebot_message_context(raw: &str, include_image_summary: bool) -> String {
    let normalized = decode_html_brackets(raw);
    if let Some(params) = extract_cq_image_params(&normalized) {
        return render_cq_image_context(&params, include_image_summary);
    }

    let text = normalized.trim();
    if text.is_empty() {
        "<empty>".to_string()
    } else {
        text.to_string()
    }
}

pub(crate) fn extract_cq_image_params(raw: &str) -> Option<HashMap<String, String>> {
    let start = raw.find("[CQ:image")?;
    let end = raw[start..].find(']')? + start;
    let block = &raw[start + 1..end];

    let mut params = HashMap::new();
    for part in block.split(',').skip(1) {
        if let Some((key, value)) = part.split_once('=') {
            params.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Some(params)
}

pub(crate) fn render_cq_image_context(
    params: &HashMap<String, String>,
    include_image_summary: bool,
) -> String {
    if !include_image_summary {
        return "[图片]".to_string();
    }

    let summary = params
        .get("summary")
        .map(|raw| {
            decode_html_brackets(raw)
                .replace(['[', ']'], "")
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty());

    match summary {
        Some(text) if text.contains("动画表情") => "[动画表情]".to_string(),
        Some(text) => format!("[图片:{text}]"),
        None => "[图片]".to_string(),
    }
}

pub(crate) fn decode_html_brackets(raw: &str) -> String {
    raw.replace("&#91;", "[").replace("&#93;", "]")
}

pub(crate) fn parse_env_bool(key: &str, default: bool) -> bool {
    let Ok(raw) = std::env::var(key) else {
        return default;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

pub(crate) fn preview_value_to_string(value: &Value) -> Option<String> {
    if let Some(raw) = value.as_str() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_u64() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_i64() {
        return Some(raw.to_string());
    }
    None
}

pub(crate) fn truncate_preview(raw: &str, max_chars: usize) -> String {
    let mut iter = raw.chars();
    let preview: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}
