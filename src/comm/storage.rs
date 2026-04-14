use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::broadcast;

use super::channel::{ChannelError, ChannelMessage, ChannelRegistry};

#[derive(Clone)]
pub struct SharedStore {
    inner: Arc<Mutex<HashMap<String, Value>>>,
    registry: ChannelRegistry,
}

impl SharedStore {
    pub fn new(registry: ChannelRegistry) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            registry,
        }
    }

    pub fn set(&self, key: impl Into<String>, value: Value) {
        self.inner.lock().unwrap().insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.inner.lock().unwrap().get(key).cloned()
    }

    pub fn delete(&self, key: &str) -> Option<Value> {
        self.inner.lock().unwrap().remove(key)
    }

    pub fn snapshot(&self) -> HashMap<String, Value> {
        self.inner.lock().unwrap().clone()
    }

    pub fn publish(&self, channel_name: &str, message: ChannelMessage) -> Result<(), ChannelError> {
        let channel = self.registry.get_or_create(channel_name, 256);
        channel.send(message)
    }

    pub fn subscribe(&self, channel_name: &str) -> broadcast::Receiver<ChannelMessage> {
        self.registry.get_or_create(channel_name, 256).subscribe()
    }
}
