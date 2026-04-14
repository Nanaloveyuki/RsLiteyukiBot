use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Value, json};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::observability::Logger;

use super::error::AdapterError;
use super::http::{HttpMethod, HttpTransportClient};
use super::model::{AdapterConfig, AdapterTransport};
use super::packet::AdapterPacket;
use super::sse::SseTransportClient;
use super::websocket::{
    AdapterSink, AdapterSinkFuture, WebSocketAdapterHandle, start_forward_adapter,
    start_reverse_adapter,
};

const MODULE_ADAPTER: &str = "adapter.manager";

pub type ManagedAdapterSink = AdapterSink;
pub type ManagedAdapterSinkFuture = AdapterSinkFuture;

enum RunningAdapter {
    WebSocket(WebSocketAdapterHandle),
    Sse {
        shutdown_tx: watch::Sender<bool>,
        task: JoinHandle<()>,
    },
    Http,
}

impl RunningAdapter {
    async fn shutdown(self) -> Result<(), AdapterError> {
        match self {
            Self::WebSocket(handle) => handle.shutdown().await,
            Self::Sse { shutdown_tx, task } => {
                let _ = shutdown_tx.send(true);
                let _ = task.await;
                Ok(())
            }
            Self::Http => Ok(()),
        }
    }

    async fn send(&self, packet: AdapterPacket) -> Result<(), AdapterError> {
        match self {
            Self::WebSocket(handle) => handle.send(packet).await,
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
pub struct AdapterManager {
    configs: Arc<RwLock<HashMap<String, AdapterConfig>>>,
    running: Arc<Mutex<HashMap<String, RunningAdapter>>>,
    http_client: HttpTransportClient,
    sse_client: SseTransportClient,
    logger: Option<Logger>,
}

impl Default for AdapterManager {
    fn default() -> Self {
        Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(Mutex::new(HashMap::new())),
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
        let mut manager = Self::default();
        manager.logger = Some(logger);
        manager
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
            .lock()
            .expect("adapter running lock should not be poisoned")
            .contains_key(id)
    }

    pub async fn start(&self, id: &str, sink: ManagedAdapterSink) -> Result<(), AdapterError> {
        {
            if self
                .running
                .lock()
                .expect("adapter running lock should not be poisoned")
                .contains_key(id)
            {
                return Ok(());
            }
        }

        let config = self
            .get(id)
            .ok_or_else(|| AdapterError::Config(format!("adapter '{}' not found", id)))?;
        if !config.enabled {
            return Ok(());
        }

        let running = match config.transport {
            AdapterTransport::WebSocketForward => {
                let handle =
                    start_forward_adapter(config.endpoint.clone(), config.queue_capacity, sink)
                        .await?;
                RunningAdapter::WebSocket(handle)
            }
            AdapterTransport::WebSocketReverse => {
                let handle =
                    start_reverse_adapter(config.endpoint.clone(), config.queue_capacity, sink)
                        .await?;
                RunningAdapter::WebSocket(handle)
            }
            AdapterTransport::Sse => {
                let mut rx = self
                    .sse_client
                    .open_stream(&config.endpoint, config.queue_capacity)
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
                RunningAdapter::Sse { shutdown_tx, task }
            }
            AdapterTransport::Http => RunningAdapter::Http,
        };

        self.running
            .lock()
            .expect("adapter running lock should not be poisoned")
            .insert(id.to_string(), running);

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
            .lock()
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
            .lock()
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
            self.http_client.post_packet(&config.endpoint, &packet).await?;
            return Ok(());
        }

        let running = self
            .running
            .lock()
            .expect("adapter running lock should not be poisoned")
            .remove(id)
            .ok_or_else(|| AdapterError::Config(format!("adapter '{}' is not running", id)))?;
        let result = running.send(packet).await;
        self.running
            .lock()
            .expect("adapter running lock should not be poisoned")
            .insert(id.to_string(), running);
        result
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
        self.http_client
            .request_json(method, &config.endpoint, body)
            .await
    }
}

pub fn sink_from_fn<F, Fut>(handler: F) -> ManagedAdapterSink
where
    F: Fn(AdapterPacket) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |packet| Box::pin(handler(packet)))
}

