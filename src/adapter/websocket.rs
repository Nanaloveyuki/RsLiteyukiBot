use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, HeaderName};

use super::error::AdapterError;
use super::model::AdapterEndpoint;
use super::packet::AdapterPacket;

pub type AdapterSinkFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub type AdapterSink = Arc<dyn Fn(AdapterPacket) -> AdapterSinkFuture + Send + Sync + 'static>;

#[derive(Clone)]
pub(crate) enum WebSocketOutboundSender {
    Forward(mpsc::Sender<AdapterPacket>),
    Reverse(broadcast::Sender<AdapterPacket>),
}

impl WebSocketOutboundSender {
    pub async fn send(&self, packet: AdapterPacket) -> Result<(), AdapterError> {
        match self {
            Self::Forward(sender) => sender
                .send(packet)
                .await
                .map_err(|err| AdapterError::WebSocket(format!("forward send failed: {}", err))),
            Self::Reverse(sender) => sender
                .send(packet)
                .map(|_| ())
                .map_err(|err| AdapterError::WebSocket(format!("reverse send failed: {}", err))),
        }
    }
}

pub struct WebSocketAdapterHandle {
    outbound: WebSocketOutboundSender,
    shutdown_tx: watch::Sender<bool>,
    task: JoinHandle<Result<(), AdapterError>>,
}

impl WebSocketAdapterHandle {
    pub async fn send(&self, packet: AdapterPacket) -> Result<(), AdapterError> {
        self.outbound.send(packet).await
    }

    pub async fn shutdown(self) -> Result<(), AdapterError> {
        let _ = self.shutdown_tx.send(true);
        self.task
            .await
            .map_err(|err| AdapterError::WebSocket(format!("adapter task join failed: {}", err)))?
    }

    pub async fn send_packet(&self, packet: AdapterPacket) -> Result<(), AdapterError> {
        self.send(packet).await
    }

    pub async fn stop(self) -> Result<(), AdapterError> {
        self.shutdown().await
    }

    pub(crate) fn outbound_sender(&self) -> WebSocketOutboundSender {
        self.outbound.clone()
    }
}

pub async fn start_forward_adapter(
    endpoint: AdapterEndpoint,
    queue_capacity: usize,
    max_payload_size: Option<usize>,
    sink: AdapterSink,
) -> Result<WebSocketAdapterHandle, AdapterError> {
    let request = to_ws_request(&endpoint)?;
    let (stream, _) = connect_async(request)
        .await
        .map_err(|err| AdapterError::WebSocket(format!("connect failed: {}", err)))?;
    let (mut write, mut read) = stream.split();

    let (tx, mut rx) = mpsc::channel::<AdapterPacket>(queue_capacity.max(1));
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                outgoing = rx.recv() => {
                    let Some(packet) = outgoing else {
                        break;
                    };
                    let text = serialize_packet(&packet)?;
                    enforce_ws_payload_limit(text.len(), max_payload_size, "forward outbound")?;
                    write.send(Message::Text(text))
                        .await
                        .map_err(|err| AdapterError::WebSocket(format!("write ws message failed: {}", err)))?;
                }
                incoming = read.next() => {
                    let Some(incoming) = incoming else {
                        break;
                    };
                    let incoming = incoming
                        .map_err(|err| AdapterError::WebSocket(format!("read ws message failed: {}", err)))?;
                    if let Some(size) = ws_message_payload_size(&incoming) {
                        enforce_ws_payload_limit(size, max_payload_size, "forward inbound")?;
                    }
                    if let Some(packet) = parse_packet(incoming)? {
                        sink(packet).await;
                    }
                }
            }
        }
        Ok(())
    });

    Ok(WebSocketAdapterHandle {
        outbound: WebSocketOutboundSender::Forward(tx),
        shutdown_tx,
        task,
    })
}

pub async fn start_reverse_adapter(
    endpoint: AdapterEndpoint,
    queue_capacity: usize,
    max_payload_size: Option<usize>,
    max_connections: Option<usize>,
    worker_parallelism: usize,
    sink: AdapterSink,
) -> Result<WebSocketAdapterHandle, AdapterError> {
    let bind_addr = parse_bind_addr(&endpoint.url)?;
    let listener = TcpListener::bind(&bind_addr)
        .await
        .map_err(|err| AdapterError::Io(format!("bind '{}' failed: {}", bind_addr, err)))?;
    let (outbound_tx, _) = broadcast::channel::<AdapterPacket>(queue_capacity.max(1));
    let outbound_for_task = outbound_tx.clone();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let active_connections = Arc::new(AtomicUsize::new(0));
    let worker_count = worker_parallelism.max(1);

    let task = tokio::spawn(async move {
        let mut worker_senders = Vec::with_capacity(worker_count);
        let mut worker_tasks: Vec<JoinHandle<()>> = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let (worker_tx, mut worker_rx) = mpsc::channel(queue_capacity.max(1));
            worker_senders.push(worker_tx);

            let mut worker_shutdown = shutdown_rx.clone();
            let sink = sink.clone();
            let outbound_for_worker = outbound_for_task.clone();
            let active_connections = Arc::clone(&active_connections);
            let max_payload_size = max_payload_size;
            let max_connections = max_connections;

            worker_tasks.push(tokio::spawn(async move {
                let mut connection_tasks: Vec<JoinHandle<()>> = Vec::new();
                loop {
                    tokio::select! {
                        changed = worker_shutdown.changed() => {
                            if changed.is_err() || *worker_shutdown.borrow() {
                                break;
                            }
                        }
                        accepted = worker_rx.recv() => {
                            let Some(socket) = accepted else {
                                break;
                            };
                            if let Some(limit) = max_connections
                                && active_connections.load(Ordering::Relaxed) >= limit
                            {
                                continue;
                            }

                            let Ok(ws_stream) = accept_async(socket).await else {
                                continue;
                            };
                            active_connections.fetch_add(1, Ordering::Relaxed);
                            let (mut write, mut read) = ws_stream.split();
                            let mut local_shutdown = worker_shutdown.clone();
                            let mut outbound_rx = outbound_for_worker.subscribe();
                            let sink = sink.clone();
                            let active_connections = Arc::clone(&active_connections);
                            let max_payload_size = max_payload_size;

                            connection_tasks.retain(|task| !task.is_finished());
                            connection_tasks.push(tokio::spawn(async move {
                                loop {
                                    tokio::select! {
                                        changed = local_shutdown.changed() => {
                                            if changed.is_err() || *local_shutdown.borrow() {
                                                break;
                                            }
                                        }
                                        outbound = outbound_rx.recv() => {
                                            let Ok(packet) = outbound else {
                                                break;
                                            };
                                            let Ok(text) = serialize_packet(&packet) else {
                                                continue;
                                            };
                                            if enforce_ws_payload_limit(text.len(), max_payload_size, "reverse outbound").is_err() {
                                                continue;
                                            }
                                            if write.send(Message::Text(text)).await.is_err() {
                                                break;
                                            }
                                        }
                                        inbound = read.next() => {
                                            let Some(inbound) = inbound else {
                                                break;
                                            };
                                            let Ok(inbound) = inbound else {
                                                break;
                                            };
                                            if let Some(size) = ws_message_payload_size(&inbound)
                                                && enforce_ws_payload_limit(size, max_payload_size, "reverse inbound").is_err()
                                            {
                                                continue;
                                            }
                                            if let Ok(Some(packet)) = parse_packet(inbound) {
                                                sink(packet).await;
                                            }
                                        }
                                    }
                                }
                                active_connections.fetch_sub(1, Ordering::Relaxed);
                            }));
                        }
                    }
                }

                for task in connection_tasks {
                    task.abort();
                }
            }));
        }

        let mut next_worker = 0usize;
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let (socket, _) = accepted
                        .map_err(|err| AdapterError::Io(format!("accept failed: {}", err)))?;
                    if worker_senders.is_empty() {
                        continue;
                    }
                    let index = next_worker % worker_senders.len();
                    next_worker = if index + 1 == worker_senders.len() {
                        0
                    } else {
                        index + 1
                    };
                    if worker_senders[index].send(socket).await.is_err() {
                        break;
                    }
                }
            }
        }

        drop(worker_senders);
        for task in worker_tasks {
            let _ = task.await;
        }
        Ok(())
    });

    Ok(WebSocketAdapterHandle {
        outbound: WebSocketOutboundSender::Reverse(outbound_tx),
        shutdown_tx,
        task,
    })
}

fn to_ws_request(
    endpoint: &AdapterEndpoint,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, AdapterError> {
    let mut request = endpoint
        .url
        .clone()
        .into_client_request()
        .map_err(|err| AdapterError::Config(format!("invalid ws url: {}", err)))?;

    let headers = request.headers_mut();
    for (key, value) in &endpoint.headers {
        let key = HeaderName::from_bytes(key.as_bytes()).map_err(|err| {
            AdapterError::Config(format!("invalid header name '{}': {}", key, err))
        })?;
        let value = HeaderValue::from_str(value)
            .map_err(|err| AdapterError::Config(format!("invalid header '{}': {}", key, err)))?;
        headers.insert(key, value);
    }

    if let Some(token) = &endpoint.token {
        let header_value = HeaderValue::from_str(&format!("Bearer {}", token)).map_err(|err| {
            AdapterError::Config(format!("invalid authorization header: {}", err))
        })?;
        headers.insert(AUTHORIZATION, header_value);
    }

    Ok(request)
}

fn parse_bind_addr(url: &str) -> Result<String, AdapterError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|err| AdapterError::Config(format!("invalid reverse ws url: {}", err)))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| AdapterError::Config("reverse ws host missing".to_string()))?;
    let port = parsed.port_or_known_default().ok_or_else(|| {
        AdapterError::Config("reverse ws port missing and cannot infer default".to_string())
    })?;
    Ok(format!("{}:{}", host, port))
}

fn serialize_packet(packet: &AdapterPacket) -> Result<String, AdapterError> {
    if let Some(payload) = onebot_v11_outbound_payload(packet) {
        return serde_json::to_string(payload).map_err(|err| {
            AdapterError::Serialize(format!("encode ws packet(onebot-v11) failed: {}", err))
        });
    }
    serde_json::to_string(packet)
        .map_err(|err| AdapterError::Serialize(format!("encode ws packet failed: {}", err)))
}

fn parse_packet(message: Message) -> Result<Option<AdapterPacket>, AdapterError> {
    match message {
        Message::Text(text) => parse_packet_text(&text),
        Message::Binary(binary) => {
            let text = String::from_utf8(binary.to_vec()).map_err(|err| {
                AdapterError::Serialize(format!("binary ws payload is not utf8: {}", err))
            })?;
            parse_packet_text(&text)
        }
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Ok(None),
        Message::Frame(_) => Ok(None),
    }
}

fn parse_packet_text(text: &str) -> Result<Option<AdapterPacket>, AdapterError> {
    let value = serde_json::from_str::<Value>(text)
        .map_err(|err| AdapterError::Serialize(format!("decode ws json failed: {}", err)))?;

    if let Ok(packet) = serde_json::from_value::<AdapterPacket>(value.clone()) {
        return Ok(Some(packet));
    }

    Ok(decode_onebot_v11_packet(value))
}

fn decode_onebot_v11_packet(value: Value) -> Option<AdapterPacket> {
    let Value::Object(mut object) = value else {
        return None;
    };

    if !looks_like_onebot_v11_payload(&object) {
        return None;
    }

    let timestamp_ms = extract_onebot_timestamp_ms(&object);
    let id = object
        .get("message_id")
        .and_then(value_to_u64)
        .or_else(|| object.get("echo").and_then(value_to_u64))
        .or_else(|| object.get("time").and_then(value_to_u64))
        .map(|value| value.to_string())
        .unwrap_or_else(|| timestamp_ms.to_string());

    object
        .entry("_adapter_protocol".to_string())
        .or_insert_with(|| Value::String("onebot.v11".to_string()));

    Some(AdapterPacket {
        id,
        topic: onebot_v11_topic(&object),
        payload: Value::Object(object),
        timestamp_ms,
        meta: Default::default(),
    })
}

fn looks_like_onebot_v11_payload(object: &Map<String, Value>) -> bool {
    object.contains_key("post_type")
        || (object.contains_key("status") && object.contains_key("retcode"))
}

fn onebot_v11_topic(object: &Map<String, Value>) -> String {
    if let Some(post_type) = object.get("post_type").and_then(Value::as_str) {
        match post_type {
            "message" => {
                let message_type = object
                    .get("message_type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                format!("onebot.v11.event.message.{}", message_type)
            }
            "notice" => {
                let notice_type = object
                    .get("notice_type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                format!("onebot.v11.event.notice.{}", notice_type)
            }
            "request" => {
                let request_type = object
                    .get("request_type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                format!("onebot.v11.event.request.{}", request_type)
            }
            "meta_event" => {
                let meta_type = object
                    .get("meta_event_type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                format!("onebot.v11.event.meta_event.{}", meta_type)
            }
            _ => format!("onebot.v11.event.{}", post_type),
        }
    } else if object.contains_key("status") && object.contains_key("retcode") {
        "onebot.v11.api.response".to_string()
    } else {
        "onebot.v11.event".to_string()
    }
}

fn extract_onebot_timestamp_ms(object: &Map<String, Value>) -> u128 {
    object
        .get("time")
        .and_then(value_to_u64)
        .map(|seconds| u128::from(seconds).saturating_mul(1000))
        .unwrap_or_else(now_millis)
}

fn value_to_u64(value: &Value) -> Option<u64> {
    if let Some(raw) = value.as_u64() {
        return Some(raw);
    }
    if let Some(raw) = value.as_i64() {
        return u64::try_from(raw).ok();
    }
    if let Some(raw) = value.as_str() {
        return raw.parse::<u64>().ok();
    }
    None
}

fn onebot_v11_outbound_payload(packet: &AdapterPacket) -> Option<&Value> {
    let object = packet.payload.as_object()?;
    if object.get("action").and_then(Value::as_str).is_some() {
        return Some(&packet.payload);
    }
    None
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
#[path = "websocket/tests.rs"]
mod tests;

fn ws_message_payload_size(message: &Message) -> Option<usize> {
    match message {
        Message::Text(text) => Some(text.len()),
        Message::Binary(binary) => Some(binary.len()),
        Message::Ping(payload) => Some(payload.len()),
        Message::Pong(payload) => Some(payload.len()),
        Message::Close(_) | Message::Frame(_) => None,
    }
}

fn enforce_ws_payload_limit(
    payload_len: usize,
    max_payload_size: Option<usize>,
    direction: &str,
) -> Result<(), AdapterError> {
    if let Some(limit) = max_payload_size
        && payload_len > limit
    {
        return Err(AdapterError::WebSocket(format!(
            "{} payload exceeded limit: {} > {} bytes",
            direction, payload_len, limit
        )));
    }
    Ok(())
}
