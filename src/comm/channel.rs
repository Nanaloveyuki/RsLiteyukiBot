use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast::{self, Receiver, Sender};

/// A message emitted within a channel, carrying origin metadata so subscribers can trace it.
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub topic: Arc<str>,
    pub payload: Value,
    pub source: Option<Arc<str>>,
    pub timestamp_ms: u128,
}

impl ChannelMessage {
    /// Build a message from JSON value.
    pub fn new<T, S>(topic: T, payload: Value, source: Option<S>) -> Self
    where
        T: Into<Arc<str>>,
        S: Into<Arc<str>>,
    {
        Self {
            topic: topic.into(),
            payload,
            source: source.map(Into::into),
            timestamp_ms: current_millis(),
        }
    }

    /// Build while serializing the payload.
    pub fn try_new<T, V, S>(topic: T, payload: V, source: Option<S>) -> Result<Self, ChannelError>
    where
        T: Into<Arc<str>>,
        V: Serialize,
        S: Into<Arc<str>>,
    {
        let value = serde_json::to_value(payload).map_err(ChannelError::Serialization)?;
        Ok(Self::new(topic, value, source))
    }
}

fn current_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

/// Errors propagated from channel operations.
#[derive(Debug)]
pub enum ChannelError {
    Send(broadcast::error::SendError<ChannelMessage>),
    Serialization(serde_json::Error),
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelError::Send(err) => write!(f, "broadcast send failed: {err}"),
            ChannelError::Serialization(err) => write!(f, "failed to serialize payload: {err}"),
        }
    }
}

impl Error for ChannelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ChannelError::Send(err) => Some(err),
            ChannelError::Serialization(err) => Some(err),
        }
    }
}

/// A lightweight tokio broadcast wrapper that exposes the channel name for debug.
#[derive(Clone)]
pub struct Channel {
    name: Arc<str>,
    inner: Sender<ChannelMessage>,
}

impl Channel {
    pub fn new(name: impl Into<Arc<str>>, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (sender, _) = broadcast::channel(capacity);
        Channel {
            name: name.into(),
            inner: sender,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn send(&self, message: ChannelMessage) -> Result<(), ChannelError> {
        self.inner
            .send(message)
            .map(|_| ())
            .map_err(ChannelError::Send)
    }

    pub fn subscribe(&self) -> Receiver<ChannelMessage> {
        self.inner.subscribe()
    }
}

/// Registry ensuring there is at most one channel per name.
#[derive(Default, Clone)]
pub struct ChannelRegistry {
    inner: Arc<Mutex<HashMap<String, Channel>>>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get_or_create(&self, name: &str, capacity: usize) -> Channel {
        let capacity = capacity.max(1);
        let mut lock = self.inner.lock().unwrap();
        lock.entry(name.to_owned())
            .or_insert_with(|| Channel::new(name, capacity))
            .clone()
    }
}
