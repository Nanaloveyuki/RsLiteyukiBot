mod error;
mod http;
mod manager;
mod model;
mod packet;
mod sse;
mod websocket;

pub use error::AdapterError;
pub use http::{HttpMethod, HttpTransportClient};
pub use manager::{AdapterManager, ManagedAdapterSink, ManagedAdapterSinkFuture, sink_from_fn};
pub use model::{AdapterConfig, AdapterEndpoint, AdapterRoute, AdapterTransport};
pub use packet::AdapterPacket;
pub use sse::{SseEvent, SseParser, SseTransportClient, decode_sse_event, encode_sse_event};
pub use websocket::{
    AdapterSink, AdapterSinkFuture, WebSocketAdapterHandle, start_forward_adapter,
    start_reverse_adapter,
};
