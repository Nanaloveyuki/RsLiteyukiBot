use std::sync::Arc;

use serde_json::Value;

use crate::core::BotEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionScope {
    Private,
    Group,
    Guild,
    ChannelText,
    ChannelCategory,
    ChannelVoice,
    Other(Arc<str>),
}

impl SessionScope {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "private" => Self::Private,
            "group" => Self::Group,
            "guild" => Self::Guild,
            "channel_text" | "channel-text" | "text" => Self::ChannelText,
            "channel_category" | "channel-category" | "category" => Self::ChannelCategory,
            "channel_voice" | "channel-voice" | "voice" => Self::ChannelVoice,
            _ => Self::Other(Arc::from(value.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub event_id: u64,
    pub topic: Arc<str>,
    pub message: Arc<str>,
    pub payload: Value,
    pub timestamp_ms: u128,
    pub bot_id: Arc<str>,
    pub session_id: Arc<str>,
    pub user_id: Arc<str>,
    pub scope: SessionScope,
}

impl SessionEvent {
    pub fn from_bot_event(event: &BotEvent) -> Self {
        let payload = event.payload.clone();
        let bot_id = payload
            .get("bot_id")
            .or_else(|| payload.get("self_id"))
            .and_then(value_to_string)
            .unwrap_or_else(|| "default".to_string());
        let user_id = payload
            .get("user_id")
            .and_then(value_to_string)
            .unwrap_or_else(|| "anonymous".to_string());
        let topic = Arc::<str>::from(event.topic.clone());

        let scope = payload
            .get("scope")
            .and_then(Value::as_str)
            .map(SessionScope::parse)
            .or_else(|| {
                payload
                    .get("message_type")
                    .and_then(Value::as_str)
                    .map(SessionScope::parse)
            })
            .unwrap_or_else(|| infer_scope(topic.as_ref()));

        let session_id = payload
            .get("session_id")
            .and_then(value_to_string)
            .or_else(|| payload.get("group_id").and_then(value_to_string))
            .or_else(|| payload.get("guild_id").and_then(value_to_string))
            .or_else(|| payload.get("channel_id").and_then(value_to_string))
            .unwrap_or_else(|| format!("{}:{}", topic, user_id));

        let message = extract_message_text(&payload).unwrap_or_default();

        Self {
            event_id: event.id,
            topic,
            message: Arc::<str>::from(message),
            payload,
            timestamp_ms: event.timestamp_ms,
            bot_id: Arc::<str>::from(bot_id),
            session_id: Arc::<str>::from(session_id),
            user_id: Arc::<str>::from(user_id),
            scope,
        }
    }
}

fn extract_message_text(payload: &Value) -> Option<String> {
    payload
        .get("text")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            payload
                .get("raw_message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .or_else(|| {
            payload
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .or_else(|| {
            payload
                .get("message")
                .and_then(Value::as_array)
                .and_then(|segments| join_message_segments(segments.as_slice()))
        })
}

fn join_message_segments(segments: &[Value]) -> Option<String> {
    let mut out = String::new();
    for segment in segments {
        if let Some(text) = segment
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("text"))
            .and_then(Value::as_str)
        {
            out.push_str(text);
            continue;
        }
        if let Some(text) = segment.get("text").and_then(Value::as_str) {
            out.push_str(text);
            continue;
        }
        if let Some(text) = segment.as_str() {
            out.push_str(text);
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

fn value_to_string(value: &Value) -> Option<String> {
    if let Some(raw) = value.as_str() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_u64() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_i64() {
        return Some(raw.to_string());
    }
    if let Some(raw) = value.as_bool() {
        return Some(raw.to_string());
    }
    None
}

fn infer_scope(topic: &str) -> SessionScope {
    let normalized = topic.to_ascii_lowercase();
    if normalized.contains("private") {
        return SessionScope::Private;
    }
    if normalized.contains("group") {
        return SessionScope::Group;
    }
    if normalized.contains("guild") {
        return SessionScope::Guild;
    }
    if normalized.contains("channel.voice") {
        return SessionScope::ChannelVoice;
    }
    if normalized.contains("channel.category") {
        return SessionScope::ChannelCategory;
    }
    if normalized.contains("channel") {
        return SessionScope::ChannelText;
    }
    SessionScope::Other(Arc::<str>::from("unknown"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::BotEvent;
    use serde_json::json;

    #[test]
    fn from_bot_event_extracts_onebot_v11_fields() {
        let event = BotEvent::new(
            1,
            "adapter.inbound",
            json!({
                "self_id": 42,
                "post_type": "message",
                "message_type": "group",
                "group_id": 123456,
                "user_id": 10001,
                "raw_message": "你好"
            }),
        );

        let session = SessionEvent::from_bot_event(&event);
        assert_eq!(session.bot_id.as_ref(), "42");
        assert_eq!(session.user_id.as_ref(), "10001");
        assert_eq!(session.session_id.as_ref(), "123456");
        assert_eq!(session.message.as_ref(), "你好");
        assert_eq!(session.scope, SessionScope::Group);
    }

    #[test]
    fn from_bot_event_joins_message_segments() {
        let event = BotEvent::new(
            2,
            "adapter.inbound",
            json!({
                "message_type": "private",
                "user_id": "u100",
                "message": [
                    { "type": "text", "data": { "text": "hello" } },
                    { "type": "text", "data": { "text": " world" } }
                ]
            }),
        );

        let session = SessionEvent::from_bot_event(&event);
        assert_eq!(session.message.as_ref(), "hello world");
        assert_eq!(session.scope, SessionScope::Private);
    }
}
