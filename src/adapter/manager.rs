use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore, watch};
use tokio::task::JoinHandle;

use crate::observability::Logger;

use super::error::AdapterError;
use super::http::{HttpMethod, HttpTransportClient};
use super::model::{AdapterConfig, AdapterTransport};
use super::packet::AdapterPacket;
use super::sse::SseTransportClient;
use super::websocket::{
    AdapterSink, AdapterSinkFuture, WebSocketAdapterHandle, WebSocketOutboundSender,
    start_forward_adapter, start_reverse_adapter,
};

const MODULE_ADAPTER: &str = "adapter.manager";

pub type ManagedAdapterSink = AdapterSink;
pub type ManagedAdapterSinkFuture = AdapterSinkFuture;

enum RunningAdapter {
    WebSocket {
        handle: WebSocketAdapterHandle,
        outbound: WebSocketOutboundSender,
    },
    Sse {
        shutdown_tx: watch::Sender<bool>,
        task: JoinHandle<()>,
    },
    Http,
}

impl RunningAdapter {
    async fn shutdown(self) -> Result<(), AdapterError> {
        match self {
            Self::WebSocket { handle, .. } => handle.shutdown().await,
            Self::Sse { shutdown_tx, task } => {
                let _ = shutdown_tx.send(true);
                let _ = task.await;
                Ok(())
            }
            Self::Http => Ok(()),
        }
    }

    fn sender(&self) -> Result<RunningAdapterSender, AdapterError> {
        match self {
            Self::WebSocket { outbound, .. } => {
                Ok(RunningAdapterSender::WebSocket(outbound.clone()))
            }
            Self::Sse { .. } => Err(AdapterError::Sse(
                "sse adapter does not support outbound packet send".to_string(),
            )),
            Self::Http => Err(AdapterError::Http(
                "http adapter outbound send should use http client path".to_string(),
            )),
        }
    }
}

#[derive(Clone)]
enum RunningAdapterSender {
    WebSocket(WebSocketOutboundSender),
}

impl RunningAdapterSender {
    async fn send(&self, packet: AdapterPacket) -> Result<(), AdapterError> {
        match self {
            Self::WebSocket(sender) => sender.send(packet).await,
        }
    }
}

struct RunningAdapterSlot {
    inner: AsyncMutex<Option<RunningAdapter>>,
}

impl RunningAdapterSlot {
    fn new(adapter: RunningAdapter) -> Self {
        Self {
            inner: AsyncMutex::new(Some(adapter)),
        }
    }

    async fn send(&self, packet: AdapterPacket) -> Result<(), AdapterError> {
        let sender = {
            let lock = self.inner.lock().await;
            let running = lock
                .as_ref()
                .ok_or_else(|| AdapterError::Config("adapter is stopping".to_string()))?;
            running.sender()?
        };
        sender.send(packet).await
    }

    async fn shutdown(&self) -> Result<(), AdapterError> {
        let running = {
            let mut lock = self.inner.lock().await;
            lock.take()
        };
        if let Some(running) = running {
            running.shutdown().await
        } else {
            Ok(())
        }
    }
}

struct StartGuard {
    id: String,
    starting: Arc<Mutex<HashSet<String>>>,
}

impl StartGuard {
    fn acquire(starting: Arc<Mutex<HashSet<String>>>, id: &str) -> Option<Self> {
        {
            let mut lock = starting
                .lock()
                .expect("adapter start-inflight lock should not be poisoned");
            if !lock.insert(id.to_string()) {
                return None;
            }
        }
        Some(Self {
            id: id.to_string(),
            starting,
        })
    }
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        let mut lock = self
            .starting
            .lock()
            .expect("adapter start-inflight lock should not be poisoned");
        lock.remove(&self.id);
    }
}

#[derive(Clone)]
pub struct AdapterManager {
    configs: Arc<RwLock<HashMap<String, AdapterConfig>>>,
    running: Arc<RwLock<HashMap<String, Arc<RunningAdapterSlot>>>>,
    starting: Arc<Mutex<HashSet<String>>>,
    http_limiters: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    http_client: HttpTransportClient,
    sse_client: SseTransportClient,
    logger: Option<Logger>,
}

impl Default for AdapterManager {
    fn default() -> Self {
        Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(RwLock::new(HashMap::new())),
            starting: Arc::new(Mutex::new(HashSet::new())),
            http_limiters: Arc::new(Mutex::new(HashMap::new())),
            http_client: HttpTransportClient::default(),
            sse_client: SseTransportClient::default(),
            logger: None,
        }
    }
}

impl AdapterManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_logger(logger: Logger) -> Self {
        Self {
            logger: Some(logger),
            ..Self::default()
        }
    }

    pub fn set_logger(&mut self, logger: Logger) {
        self.logger = Some(logger);
    }

    pub fn register(&self, config: AdapterConfig) -> Result<(), AdapterError> {
        config
            .validate()
            .map_err(|err| AdapterError::Config(err.to_string()))?;
        let mut lock = self
            .configs
            .write()
            .expect("adapter config lock should not be poisoned");
        if lock.contains_key(&config.id) {
            return Err(AdapterError::Config(format!(
                "adapter '{}' already exists",
                config.id
            )));
        }
        if let Some(logger) = &self.logger {
            logger.info_in(
                MODULE_ADAPTER,
                format!("register adapter '{}' ({:?})", config.id, config.transport),
            );
        }
        lock.insert(config.id.clone(), config);
        Ok(())
    }

    fn http_limiter(&self, id: &str, max_connections: usize) -> Arc<Semaphore> {
        let mut lock = self
            .http_limiters
            .lock()
            .expect("adapter http limiter lock should not be poisoned");
        lock.entry(id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(max_connections.max(1))))
            .clone()
    }

    pub fn replace_configs<I>(&self, configs: I) -> Result<(), AdapterError>
    where
        I: IntoIterator<Item = AdapterConfig>,
    {
        let mut next = HashMap::new();
        for config in configs {
            config
                .validate()
                .map_err(|err| AdapterError::Config(err.to_string()))?;
            if next.insert(config.id.clone(), config).is_some() {
                return Err(AdapterError::Config(
                    "duplicated adapter id in reload".to_string(),
                ));
            }
        }

        let mut lock = self
            .configs
            .write()
            .expect("adapter config lock should not be poisoned");
        lock.clear();
        for (id, config) in next {
            lock.insert(id, config);
        }

        if let Some(logger) = &self.logger {
            logger.info_in(
                MODULE_ADAPTER,
                format!("adapter configs replaced, total={}", lock.len()),
            );
        }

        let mut limiter_lock = self
            .http_limiters
            .lock()
            .expect("adapter http limiter lock should not be poisoned");
        limiter_lock.retain(|id, _| lock.contains_key(id));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<AdapterConfig> {
        self.configs
            .read()
            .expect("adapter config lock should not be poisoned")
            .get(id)
            .cloned()
    }

    pub fn list(&self) -> Vec<AdapterConfig> {
        self.configs
            .read()
            .expect("adapter config lock should not be poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running
            .read()
            .expect("adapter running lock should not be poisoned")
            .contains_key(id)
    }

    pub async fn start(&self, id: &str, sink: ManagedAdapterSink) -> Result<(), AdapterError> {
        if self.is_running(id) {
            return Ok(());
        }
        let _start_guard = match StartGuard::acquire(Arc::clone(&self.starting), id) {
            Some(guard) => guard,
            None => return Ok(()),
        };
        if self.is_running(id) {
            return Ok(());
        }

        let config = self
            .get(id)
            .ok_or_else(|| AdapterError::Config(format!("adapter '{}' not found", id)))?;
        if !config.enabled {
            return Ok(());
        }

        let inbound_sink =
            with_inbound_topic(sink, config.route.inbound_topic.clone(), config.id.clone());
        let running = self.start_with_config(&config, inbound_sink).await?;

        self.running
            .write()
            .expect("adapter running lock should not be poisoned")
            .insert(id.to_string(), Arc::new(RunningAdapterSlot::new(running)));

        if let Some(logger) = &self.logger {
            logger.info_in(MODULE_ADAPTER, format!("adapter '{}' started", id));
        }
        Ok(())
    }

    pub async fn start_enabled(&self, sink: ManagedAdapterSink) -> Result<(), AdapterError> {
        let ids: Vec<String> = self
            .configs
            .read()
            .expect("adapter config lock should not be poisoned")
            .values()
            .filter(|config| config.enabled)
            .map(|config| config.id.clone())
            .collect();
        for id in ids {
            self.start(&id, sink.clone()).await?;
        }
        Ok(())
    }

    pub async fn shutdown(&self, id: &str) -> Result<(), AdapterError> {
        let running = self
            .running
            .write()
            .expect("adapter running lock should not be poisoned")
            .remove(id);
        let Some(running) = running else {
            return Ok(());
        };
        running.shutdown().await?;
        if let Some(logger) = &self.logger {
            logger.info_in(MODULE_ADAPTER, format!("adapter '{}' stopped", id));
        }
        Ok(())
    }

    pub async fn shutdown_all(&self) -> Result<(), AdapterError> {
        let ids: Vec<String> = self
            .running
            .read()
            .expect("adapter running lock should not be poisoned")
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.shutdown(&id).await?;
        }
        Ok(())
    }

    pub async fn send(&self, id: &str, packet: AdapterPacket) -> Result<(), AdapterError> {
        let config = self
            .get(id)
            .ok_or_else(|| AdapterError::Config(format!("adapter '{}' not found", id)))?;

        if matches!(config.transport, AdapterTransport::Http) {
            let _permit = self.acquire_http_permit(&config).await?;
            self.http_client
                .post_packet(&config.endpoint, &packet, config.max_payload_size)
                .await?;
            return Ok(());
        }

        let running = self
            .running
            .read()
            .expect("adapter running lock should not be poisoned")
            .get(id)
            .cloned()
            .ok_or_else(|| AdapterError::Config(format!("adapter '{}' is not running", id)))?;
        running.send(packet).await
    }

    pub async fn request_http_json(
        &self,
        id: &str,
        method: HttpMethod,
        body: Option<&Value>,
    ) -> Result<Value, AdapterError> {
        let config = self
            .get(id)
            .ok_or_else(|| AdapterError::Config(format!("adapter '{}' not found", id)))?;
        if !matches!(config.transport, AdapterTransport::Http) {
            return Err(AdapterError::Config(format!(
                "adapter '{}' is not http transport",
                id
            )));
        }
        let _permit = self.acquire_http_permit(&config).await?;
        self.http_client
            .request_json(method, &config.endpoint, body, config.max_payload_size)
            .await
    }

    async fn start_with_config(
        &self,
        config: &AdapterConfig,
        sink: ManagedAdapterSink,
    ) -> Result<RunningAdapter, AdapterError> {
        match config.transport {
            AdapterTransport::WebSocketForward => {
                let handle = start_forward_adapter(
                    config.endpoint.clone(),
                    config.queue_capacity,
                    config.max_payload_size,
                    sink,
                )
                .await?;
                let outbound = handle.outbound_sender();
                Ok(RunningAdapter::WebSocket { handle, outbound })
            }
            AdapterTransport::WebSocketReverse => {
                let handle = start_reverse_adapter(
                    config.endpoint.clone(),
                    config.queue_capacity,
                    config.max_payload_size,
                    config.max_connections,
                    sink,
                )
                .await?;
                let outbound = handle.outbound_sender();
                Ok(RunningAdapter::WebSocket { handle, outbound })
            }
            AdapterTransport::Sse => {
                let mut rx = self
                    .sse_client
                    .open_stream(
                        &config.endpoint,
                        config.queue_capacity,
                        config.max_payload_size,
                    )
                    .await?;
                let inbound_topic = config.route.inbound_topic.clone();
                let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
                let task = tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            changed = shutdown_rx.changed() => {
                                if changed.is_err() || *shutdown_rx.borrow() {
                                    break;
                                }
                            }
                            event = rx.recv() => {
                                let Some(event) = event else {
                                    break;
                                };
                                let packet = AdapterPacket::new(
                                    event.id.clone().unwrap_or_else(|| "sse-event".to_string()),
                                    inbound_topic.clone(),
                                    json!({
                                        "event": event.event,
                                        "data": event.data,
                                        "id": event.id,
                                        "retry": event.retry
                                    }),
                                );
                                sink(packet).await;
                            }
                        }
                    }
                });
                Ok(RunningAdapter::Sse { shutdown_tx, task })
            }
            AdapterTransport::Http => Ok(RunningAdapter::Http),
        }
    }

    async fn acquire_http_permit(
        &self,
        config: &AdapterConfig,
    ) -> Result<Option<OwnedSemaphorePermit>, AdapterError> {
        let Some(max_connections) = config.max_connections else {
            return Ok(None);
        };

        let permit = self
            .http_limiter(&config.id, max_connections)
            .acquire_owned()
            .await
            .map_err(|_| {
                AdapterError::Http(format!(
                    "http adapter '{}' limiter is closed unexpectedly",
                    config.id
                ))
            })?;
        Ok(Some(permit))
    }
}

pub fn sink_from_fn<F, Fut>(handler: F) -> ManagedAdapterSink
where
    F: Fn(AdapterPacket) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |packet| Box::pin(handler(packet)))
}

fn with_inbound_topic(
    sink: ManagedAdapterSink,
    inbound_topic: String,
    adapter_id: String,
) -> ManagedAdapterSink {
    Arc::new(move |packet| {
        let sink = sink.clone();
        let inbound_topic = inbound_topic.clone();
        let adapter_id = adapter_id.clone();
        Box::pin(async move {
            let packet = normalize_inbound_packet(packet, &inbound_topic, &adapter_id);
            sink(packet).await;
        })
    })
}

fn normalize_inbound_packet(
    mut packet: AdapterPacket,
    inbound_topic: &str,
    adapter_id: &str,
) -> AdapterPacket {
    match packet.payload {
        Value::Object(mut object) => {
            object
                .entry("_adapter_id".to_string())
                .or_insert_with(|| Value::String(adapter_id.to_string()));

            if should_override_inbound_topic(&packet.topic) && packet.topic != inbound_topic {
                let original_topic = packet.topic.clone();
                packet.topic = inbound_topic.to_string();
                object
                    .entry("_adapter_topic".to_string())
                    .or_insert_with(|| Value::String(original_topic));
            }
            object
                .entry("_adapter_ingress".to_string())
                .or_insert_with(|| Value::String(inbound_topic.to_string()));
            packet.payload = Value::Object(object);
        }
        other => {
            packet.payload = json!({
                "_adapter_id": adapter_id,
                "_adapter_topic": packet.topic.clone(),
                "_adapter_ingress": inbound_topic,
                "data": other
            });
            if should_override_inbound_topic(&packet.topic) && packet.topic != inbound_topic {
                packet.topic = inbound_topic.to_string();
            }
        }
    }

    packet
}

fn should_override_inbound_topic(topic: &str) -> bool {
    topic.starts_with("onebot.v11.")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalize_inbound_packet_rewrites_onebot_topic_and_injects_metadata() {
        let source_topic = "onebot.v11.event.message.group";
        let packet = AdapterPacket::new(
            "evt-1",
            source_topic,
            json!({
                "post_type": "message",
                "message_type": "group"
            }),
        );

        let normalized = normalize_inbound_packet(packet, "adapter.inbound", "ws-main");
        assert_eq!(normalized.topic, "adapter.inbound");
        assert_eq!(
            normalized
                .payload
                .get("_adapter_id")
                .and_then(Value::as_str),
            Some("ws-main")
        );
        assert_eq!(
            normalized
                .payload
                .get("_adapter_topic")
                .and_then(Value::as_str),
            Some(source_topic)
        );
        assert_eq!(
            normalized
                .payload
                .get("_adapter_ingress")
                .and_then(Value::as_str),
            Some("adapter.inbound")
        );
    }

    #[test]
    fn normalize_inbound_packet_keeps_existing_adapter_id_for_object_payload() {
        let packet = AdapterPacket::new(
            "evt-2",
            "custom.topic",
            json!({
                "_adapter_id": "upstream",
                "foo": "bar"
            }),
        );

        let normalized = normalize_inbound_packet(packet, "adapter.inbound", "local");
        assert_eq!(normalized.topic, "custom.topic");
        assert_eq!(
            normalized
                .payload
                .get("_adapter_id")
                .and_then(Value::as_str),
            Some("upstream")
        );
        assert!(
            normalized.payload.get("_adapter_topic").is_none(),
            "non-onebot topic should not add adapter_topic for object payload"
        );
    }

    #[test]
    fn normalize_inbound_packet_wraps_non_object_payload() {
        let packet = AdapterPacket::new("evt-3", "onebot.v11.event.notice", json!("raw-data"));

        let normalized = normalize_inbound_packet(packet, "adapter.inbound", "sse-main");
        assert_eq!(normalized.topic, "adapter.inbound");
        assert_eq!(
            normalized
                .payload
                .get("_adapter_id")
                .and_then(Value::as_str),
            Some("sse-main")
        );
        assert_eq!(
            normalized
                .payload
                .get("_adapter_topic")
                .and_then(Value::as_str),
            Some("onebot.v11.event.notice")
        );
        assert_eq!(normalized.payload.get("data"), Some(&json!("raw-data")));
    }
}
