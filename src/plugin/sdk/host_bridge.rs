use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::adapter::{AdapterManager, AdapterPacket};
use crate::comm::{ChannelMessage, ChannelRegistry, SharedStore};
use crate::core::LifecycleContext;
use crate::observability::Logger;
use crate::session::SessionRouter;

use super::PluginSdkError;
use super::host_async::PluginHostAsyncExecutor;

const HOST_PLUGIN_API_VERSION: &str = "0.1";

pub type PluginSdkFuture<T> = Pin<Box<dyn Future<Output = Result<T, PluginSdkError>> + Send>>;

#[derive(Debug, Clone, Default)]
pub struct PluginWebApiRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub peer_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginWebApiResponse {
    pub status_code: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub trait PluginHostApi: Send + Sync {
    fn log(&self, message: String) -> PluginSdkFuture<()>;
    fn publish(&self, channel_name: String, topic: String, payload: Value) -> PluginSdkFuture<()>;
    fn kv_get(&self, key: String) -> PluginSdkFuture<Option<Value>>;
    fn kv_set(&self, key: String, value: Value) -> PluginSdkFuture<()>;
    fn host_app_version(&self) -> &str;
    fn host_api_version(&self) -> &'static str;
}

#[derive(Clone)]
pub struct PluginHostBridge {
    lifecycle: Arc<LifecycleContext>,
    channels: ChannelRegistry,
    shared_store: SharedStore,
    session_router: SessionRouter,
    adapter_manager: AdapterManager,
    logger: Logger,
    async_executor: PluginHostAsyncExecutor,
}

impl PluginHostBridge {
    pub fn new(
        lifecycle: Arc<LifecycleContext>,
        channels: ChannelRegistry,
        shared_store: SharedStore,
        session_router: SessionRouter,
        adapter_manager: AdapterManager,
        logger: Logger,
    ) -> Self {
        Self {
            lifecycle,
            channels,
            shared_store,
            session_router,
            adapter_manager,
            logger,
            async_executor: PluginHostAsyncExecutor::new(),
        }
    }

    pub fn lifecycle(&self) -> Arc<LifecycleContext> {
        self.lifecycle.clone()
    }

    pub fn channels(&self) -> &ChannelRegistry {
        &self.channels
    }

    pub fn shared_store(&self) -> &SharedStore {
        &self.shared_store
    }

    pub fn session_router(&self) -> &SessionRouter {
        &self.session_router
    }

    pub fn adapter_manager(&self) -> &AdapterManager {
        &self.adapter_manager
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn reply_onebot_text(
        &self,
        event: &Value,
        message: &str,
        plugin_id: &str,
    ) -> Result<bool, String> {
        let text = message.trim();
        if text.is_empty() {
            return Ok(false);
        }
        let payload = event
            .get("payload")
            .and_then(Value::as_object)
            .ok_or_else(|| "event payload is missing for onebot reply".to_string())?;
        let adapter_id = payload
            .get("_adapter_id")
            .and_then(value_to_string)
            .ok_or_else(|| "event payload missing _adapter_id".to_string())?;

        let send_payload = build_onebot_v11_text_reply_payload(payload, text)
            .ok_or_else(|| "event payload is not a supported onebot message event".to_string())?;
        let packet_id = format!("plugin-{}-{}", plugin_id, now_millis());
        let adapter_manager = self.adapter_manager.clone();
        let logger = self.logger.clone();
        let plugin_id = plugin_id.to_string();
        let packet = AdapterPacket::new(packet_id, "onebot.v11.api.send_msg", send_payload);

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let adapter_manager = adapter_manager.clone();
            let logger = logger.clone();
            let plugin_id = plugin_id.clone();
            let adapter_id = adapter_id.clone();
            let packet = packet.clone();
            handle.spawn(async move {
                if let Err(err) = adapter_manager.send(&adapter_id, packet).await {
                    logger.warn_in(
                        "plugin.python",
                        format!(
                            "plugin '{}' onebot reply send failed (adapter={}): {}",
                            plugin_id, adapter_id, err
                        ),
                    );
                }
            });
        } else {
            self.async_executor.dispatch_onebot_reply(
                adapter_manager,
                adapter_id,
                packet,
                logger,
                plugin_id,
            )?;
        }
        Ok(true)
    }
}

impl PluginHostApi for PluginHostBridge {
    fn log(&self, message: String) -> PluginSdkFuture<()> {
        let logger = self.logger.clone();
        Box::pin(async move {
            logger.info_in("plugin.host", message);
            Ok(())
        })
    }

    fn publish(&self, channel_name: String, topic: String, payload: Value) -> PluginSdkFuture<()> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move {
            let message = ChannelMessage::new(topic, payload, Some("plugin-sdk"));
            shared_store
                .publish(&channel_name, message)
                .map_err(|err| PluginSdkError::Host(err.to_string()))?;
            Ok(())
        })
    }

    fn kv_get(&self, key: String) -> PluginSdkFuture<Option<Value>> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move { Ok(shared_store.get(&key)) })
    }

    fn kv_set(&self, key: String, value: Value) -> PluginSdkFuture<()> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move {
            shared_store.set(key, value);
            Ok(())
        })
    }

    fn host_app_version(&self) -> &str {
        self.lifecycle.app_version()
    }

    fn host_api_version(&self) -> &'static str {
        HOST_PLUGIN_API_VERSION
    }
}

pub(super) fn default_host_api_version() -> &'static str {
    HOST_PLUGIN_API_VERSION
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

fn build_onebot_v11_text_reply_payload(
    event_payload: &serde_json::Map<String, Value>,
    text: &str,
) -> Option<Value> {
    let message_type = event_payload
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("private")
        .to_ascii_lowercase();
    let echo = format!("plugin-reply-{}", now_millis());
    let mut params = serde_json::Map::new();
    params.insert(
        "message_type".to_string(),
        Value::String(message_type.clone()),
    );
    params.insert("message".to_string(), Value::String(text.to_string()));
    params.insert("auto_escape".to_string(), Value::Bool(false));

    match message_type.as_str() {
        "group" => {
            params.insert(
                "group_id".to_string(),
                event_payload.get("group_id")?.clone(),
            );
        }
        _ => {
            params.insert("user_id".to_string(), event_payload.get("user_id")?.clone());
            params.insert(
                "message_type".to_string(),
                Value::String("private".to_string()),
            );
        }
    }
    Some(Value::Object(serde_json::Map::from_iter([
        ("action".to_string(), Value::String("send_msg".to_string())),
        ("params".to_string(), Value::Object(params)),
        ("echo".to_string(), Value::String(echo)),
    ])))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
#[path = "host_bridge/tests.rs"]
mod tests;
