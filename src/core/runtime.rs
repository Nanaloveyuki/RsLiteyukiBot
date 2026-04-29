use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Duration, timeout};

use super::formatting::format_event_text;
use crate::observability::{Logger, LoggerConfig};

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type EventHandler = Arc<dyn Fn(BotEvent, Logger) -> BoxFuture + Send + Sync + 'static>;
const MODULE_RUNTIME: &str = "core.runtime";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotEvent {
    pub id: u64,
    pub topic: String,
    pub payload: Value,
    pub timestamp_ms: u128,
}

impl BotEvent {
    pub fn new(id: u64, topic: impl Into<String>, payload: Value) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        Self {
            id,
            topic: topic.into(),
            payload,
            timestamp_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BotRuntimeConfig {
    pub worker_count: usize,
    pub ingress_queue: usize,
    pub worker_queue: usize,
    pub logger: LoggerConfig,
}

impl Default for BotRuntimeConfig {
    fn default() -> Self {
        Self {
            worker_count: 4,
            ingress_queue: 1024,
            worker_queue: 256,
            logger: LoggerConfig::default(),
        }
    }
}

#[derive(Clone)]
pub struct BotRuntime {
    config: BotRuntimeConfig,
    logger: Logger,
    handler: EventHandler,
}

impl BotRuntime {
    pub fn new(config: BotRuntimeConfig) -> Self {
        Self::with_handler(config, |event, logger| async move {
            logger.info_in(MODULE_RUNTIME, format_event_text(&event));
        })
    }

    pub fn with_handler<F, Fut>(config: BotRuntimeConfig, handler: F) -> Self
    where
        F: Fn(BotEvent, Logger) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let logger = Logger::with_config(config.logger.clone());
        let handler: EventHandler = Arc::new(move |event, logger| Box::pin(handler(event, logger)));
        Self {
            config,
            logger,
            handler,
        }
    }

    pub fn logger(&self) -> Logger {
        self.logger.clone()
    }

    pub fn start(&self) -> BotHandle {
        let worker_count = self.config.worker_count.max(1);
        let ingress_queue = self.config.ingress_queue.max(1);
        let worker_queue = self.config.worker_queue.max(1);

        let (ingress_tx, mut ingress_rx) = mpsc::channel::<BotEvent>(ingress_queue);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let mut worker_senders = Vec::with_capacity(worker_count);
        let mut join_handles = Vec::with_capacity(worker_count + 1);

        for worker_id in 0..worker_count {
            let (worker_tx, mut worker_rx) = mpsc::channel::<BotEvent>(worker_queue);
            worker_senders.push(worker_tx);

            let mut worker_shutdown = shutdown_rx.clone();
            let logger = self.logger.clone();
            let handler = Arc::clone(&self.handler);

            let join = tokio::spawn(async move {
                logger.debug_in(MODULE_RUNTIME, format!("worker-{} started", worker_id));
                loop {
                    tokio::select! {
                        changed = worker_shutdown.changed() => {
                            match changed {
                                Ok(_) => {
                                    if *worker_shutdown.borrow() {
                                        logger.debug_in(MODULE_RUNTIME, format!("worker-{} received shutdown signal", worker_id));
                                        break;
                                    }
                                }
                                Err(_) => {
                                    logger.debug_in(MODULE_RUNTIME, format!("worker-{} shutdown channel closed", worker_id));
                                    break;
                                }
                            }
                        }
                        event = worker_rx.recv() => {
                            match event {
                                Some(event) => {
                                    (handler)(event, logger.clone()).await;
                                }
                                None => {
                                    logger.debug_in(MODULE_RUNTIME, format!("worker-{} channel closed", worker_id));
                                    break;
                                }
                            }
                        }
                    }
                }
                logger.debug_in(MODULE_RUNTIME, format!("worker-{} stopped", worker_id));
            });

            join_handles.push(join);
        }

        let mut dispatcher_shutdown = shutdown_rx.clone();
        let logger = self.logger.clone();
        let dispatcher_join = tokio::spawn(async move {
            logger.info_in(
                MODULE_RUNTIME,
                format!("runtime started with {} workers", worker_count),
            );
            let mut next_worker = 0usize;

            loop {
                tokio::select! {
                    changed = dispatcher_shutdown.changed() => {
                        match changed {
                            Ok(_) => {
                                if *dispatcher_shutdown.borrow() {
                                    logger.info_in(MODULE_RUNTIME, "dispatcher received shutdown signal");
                                    break;
                                }
                            }
                            Err(_) => {
                                logger.info_in(MODULE_RUNTIME, "dispatcher shutdown channel closed");
                                break;
                            }
                        }
                    }
                    event = ingress_rx.recv() => {
                        match event {
                            Some(event) => {
                                if let Err(event) = dispatch_event_round_robin(
                                    event,
                                    &worker_senders,
                                    &mut next_worker,
                                    &logger,
                                ) {
                                    logger.warn_in(MODULE_RUNTIME, format!(
                                        "all workers busy, dropping event id={} topic={}",
                                        event.id, event.topic
                                    ));
                                }
                            }
                            None => {
                                logger.info_in(MODULE_RUNTIME, "ingress channel closed");
                                break;
                            }
                        }
                    }
                }
            }

            drop(worker_senders);
            logger.info_in(MODULE_RUNTIME, "dispatcher stopped");
        });
        join_handles.push(dispatcher_join);

        BotHandle {
            ingress_tx,
            shutdown_tx,
            join_handles,
            logger: self.logger.clone(),
        }
    }
}

#[inline]
fn next_worker_index(current: usize, len: usize) -> usize {
    if current + 1 == len { 0 } else { current + 1 }
}

fn dispatch_event_round_robin(
    mut event: BotEvent,
    worker_senders: &[mpsc::Sender<BotEvent>],
    next_worker: &mut usize,
    logger: &Logger,
) -> Result<(), BotEvent> {
    let worker_len = worker_senders.len();
    if worker_len == 0 {
        return Err(event);
    }

    let start = *next_worker % worker_len;
    match worker_senders[start].try_send(event) {
        Ok(_) => {
            *next_worker = next_worker_index(start, worker_len);
            return Ok(());
        }
        Err(TrySendError::Full(returned)) => {
            event = returned;
        }
        Err(TrySendError::Closed(returned)) => {
            event = returned;
            logger.warn_in(MODULE_RUNTIME, format!("worker-{} closed", start));
        }
    }

    let mut index = next_worker_index(start, worker_len);
    while index != start {
        match worker_senders[index].try_send(event) {
            Ok(_) => {
                *next_worker = next_worker_index(index, worker_len);
                return Ok(());
            }
            Err(TrySendError::Full(returned)) => {
                event = returned;
            }
            Err(TrySendError::Closed(returned)) => {
                event = returned;
                logger.warn_in(MODULE_RUNTIME, format!("worker-{} closed", index));
            }
        }
        index = next_worker_index(index, worker_len);
    }

    Err(event)
}

pub struct BotHandle {
    ingress_tx: mpsc::Sender<BotEvent>,
    shutdown_tx: watch::Sender<bool>,
    join_handles: Vec<JoinHandle<()>>,
    logger: Logger,
}

impl BotHandle {
    pub fn ingress_sender(&self) -> mpsc::Sender<BotEvent> {
        self.ingress_tx.clone()
    }

    pub async fn send(&self, event: BotEvent) -> Result<(), mpsc::error::SendError<BotEvent>> {
        self.ingress_tx.send(event).await
    }

    pub async fn shutdown(self) {
        let BotHandle {
            ingress_tx,
            shutdown_tx,
            join_handles,
            logger,
        } = self;

        logger.info_in(MODULE_RUNTIME, "runtime shutdown begin");
        let _ = shutdown_tx.send(true);
        drop(ingress_tx);

        let mut join_set = JoinSet::new();
        for mut join in join_handles {
            let logger = logger.clone();
            join_set.spawn(async move {
                match timeout(Duration::from_secs(3), &mut join).await {
                    Ok(join_result) => {
                        if let Err(err) = join_result {
                            logger.error_in(MODULE_RUNTIME, format!("task join error: {}", err));
                        }
                    }
                    Err(_) => {
                        logger.warn_in(MODULE_RUNTIME, "task shutdown timeout, aborting");
                        join.abort();
                        let _ = join.await;
                    }
                }
            });
        }

        while let Some(result) = join_set.join_next().await {
            if let Err(err) = result {
                logger.error_in(
                    MODULE_RUNTIME,
                    format!("shutdown task orchestration failed: {}", err),
                );
            }
        }

        logger.info_in(MODULE_RUNTIME, "runtime shutdown complete");
    }
}

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
