use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_util::{SinkExt, StreamExt};
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

enum OutboundSender {
    Forward(mpsc::Sender<AdapterPacket>),
    Reverse(broadcast::Sender<AdapterPacket>),
}

pub struct WebSocketAdapterHandle {
    outbound: OutboundSender,
    shutdown_tx: watch::Sender<bool>,
    task: JoinHandle<Result<(), AdapterError>>,
}

impl WebSocketAdapterHandle {
    pub async fn send(&self, packet: AdapterPacket) -> Result<(), AdapterError> {
        match &self.outbound {
            OutboundSender::Forward(sender) => sender
                .send(packet)
                .await
                .map_err(|err| AdapterError::WebSocket(format!("forward send failed: {}", err))),
            OutboundSender::Reverse(sender) => sender
                .send(packet)
                .map(|_| ())
                .map_err(|err| AdapterError::WebSocket(format!("reverse send failed: {}", err))),
        }
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
        outbound: OutboundSender::Forward(tx),
        shutdown_tx,
        task,
    })
}

pub async fn start_reverse_adapter(
    endpoint: AdapterEndpoint,
    queue_capacity: usize,
    max_payload_size: Option<usize>,
    max_connections: Option<usize>,
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

    let task = tokio::spawn(async move {
        let mut connection_tasks: Vec<JoinHandle<()>> = Vec::new();
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

                    if let Some(limit) = max_connections
                        && active_connections.load(Ordering::Relaxed) >= limit
                    {
                        continue;
                    }

                    let ws_stream = accept_async(socket)
                        .await
                        .map_err(|err| AdapterError::WebSocket(format!("upgrade failed: {}", err)))?;
                    active_connections.fetch_add(1, Ordering::Relaxed);
                    let (mut write, mut read) = ws_stream.split();
                    let mut local_shutdown = shutdown_rx.clone();
                    let mut outbound_rx = outbound_for_task.subscribe();
                    let sink = sink.clone();
                    let active_connections = Arc::clone(&active_connections);
                    let max_payload_size = max_payload_size;

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
        Ok(())
    });

    Ok(WebSocketAdapterHandle {
        outbound: OutboundSender::Reverse(outbound_tx),
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
    serde_json::to_string(packet)
        .map_err(|err| AdapterError::Serialize(format!("encode ws packet failed: {}", err)))
}

fn parse_packet(message: Message) -> Result<Option<AdapterPacket>, AdapterError> {
    match message {
        Message::Text(text) => {
            let packet = serde_json::from_str::<AdapterPacket>(&text).map_err(|err| {
                AdapterError::Serialize(format!("decode ws packet failed: {}", err))
            })?;
            Ok(Some(packet))
        }
        Message::Binary(binary) => {
            let text = String::from_utf8(binary).map_err(|err| {
                AdapterError::Serialize(format!("binary ws payload is not utf8: {}", err))
            })?;
            let packet = serde_json::from_str::<AdapterPacket>(&text).map_err(|err| {
                AdapterError::Serialize(format!("decode ws packet failed: {}", err))
            })?;
            Ok(Some(packet))
        }
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Ok(None),
        Message::Frame(_) => Ok(None),
    }
}

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
