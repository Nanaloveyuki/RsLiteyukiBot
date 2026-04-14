use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterTransport {
    #[serde(alias = "websocket_forward", alias = "ws_forward")]
    WebSocketForward,
    #[serde(alias = "websocket_reverse", alias = "ws_reverse")]
    WebSocketReverse,
    Sse,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterEndpoint {
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

impl AdapterEndpoint {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.max(10))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRoute {
    #[serde(default = "default_inbound_topic")]
    pub inbound_topic: String,
    #[serde(default = "default_outbound_topic")]
    pub outbound_topic: String,
}

impl Default for AdapterRoute {
    fn default() -> Self {
        Self {
            inbound_topic: default_inbound_topic(),
            outbound_topic: default_outbound_topic(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterConfig {
    pub id: String,
    pub enabled: bool,
    pub transport: AdapterTransport,
    pub endpoint: AdapterEndpoint,
    #[serde(default)]
    pub route: AdapterRoute,
    #[serde(default = "default_queue_capacity")]
    pub queue_capacity: usize,
}

impl AdapterConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("adapter id should not be empty".to_string());
        }
        if self.endpoint.url.trim().is_empty() {
            return Err(format!("adapter '{}' url should not be empty", self.id));
        }
        if self.queue_capacity == 0 {
            return Err(format!(
                "adapter '{}' queue_capacity must be greater than zero",
                self.id
            ));
        }
        Ok(())
    }
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            id: "default-adapter".to_string(),
            enabled: true,
            transport: AdapterTransport::Http,
            endpoint: AdapterEndpoint {
                url: "http://127.0.0.1:8080/".to_string(),
                headers: HashMap::new(),
                token: None,
                timeout_ms: default_timeout_ms(),
            },
            route: AdapterRoute::default(),
            queue_capacity: default_queue_capacity(),
        }
    }
}

fn default_timeout_ms() -> u64 {
    5_000
}

fn default_queue_capacity() -> usize {
    256
}

fn default_inbound_topic() -> String {
    "adapter.inbound".to_string()
}

fn default_outbound_topic() -> String {
    "adapter.outbound".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_transport_accepts_websocket_aliases() {
        let forward: AdapterTransport =
            serde_json::from_str("\"websocket_forward\"").expect("forward alias should parse");
        assert_eq!(forward, AdapterTransport::WebSocketForward);

        let reverse: AdapterTransport =
            serde_json::from_str("\"websocket_reverse\"").expect("reverse alias should parse");
        assert_eq!(reverse, AdapterTransport::WebSocketReverse);
    }
}
