use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::BotEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterPacket {
    pub id: String,
    pub topic: String,
    pub payload: Value,
    pub timestamp_ms: u128,
    #[serde(default)]
    pub meta: HashMap<String, String>,
}

impl AdapterPacket {
    pub fn new(id: impl Into<String>, topic: impl Into<String>, payload: Value) -> Self {
        Self {
            id: id.into(),
            topic: topic.into(),
            payload,
            timestamp_ms: now_millis(),
            meta: HashMap::new(),
        }
    }

    pub fn into_bot_event(self, fallback_id: u64) -> BotEvent {
        let event_id = self.id.parse::<u64>().unwrap_or(fallback_id);
        BotEvent {
            id: event_id,
            topic: self.topic,
            payload: self.payload,
            timestamp_ms: self.timestamp_ms,
        }
    }
}

impl From<BotEvent> for AdapterPacket {
    fn from(value: BotEvent) -> Self {
        Self {
            id: value.id.to_string(),
            topic: value.topic,
            payload: value.payload,
            timestamp_ms: value.timestamp_ms,
            meta: HashMap::new(),
        }
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
