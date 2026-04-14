use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::error::AdapterError;
use super::model::AdapterEndpoint;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub id: Option<String>,
    pub retry: Option<u64>,
    pub data: String,
}

pub fn encode_sse_event(event: &SseEvent) -> String {
    let mut out = String::new();
    if let Some(id) = &event.id {
        out.push_str("id: ");
        out.push_str(id);
        out.push('\n');
    }
    if let Some(event_name) = &event.event {
        out.push_str("event: ");
        out.push_str(event_name);
        out.push('\n');
    }
    if let Some(retry) = event.retry {
        out.push_str("retry: ");
        out.push_str(&retry.to_string());
        out.push('\n');
    }
    for line in event.data.lines() {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out
}

pub fn decode_sse_event(block: &str) -> Option<SseEvent> {
    let mut event = SseEvent::default();
    let mut has_data = false;

    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field.trim(), value.trim_start()),
            None => (line.trim(), ""),
        };

        match field {
            "event" => event.event = Some(value.to_string()),
            "id" => event.id = Some(value.to_string()),
            "retry" => {
                if let Ok(value) = value.parse::<u64>() {
                    event.retry = Some(value);
                }
            }
            "data" => {
                if has_data {
                    event.data.push('\n');
                }
                event.data.push_str(value);
                has_data = true;
            }
            _ => {}
        }
    }

    if has_data { Some(event) } else { None }
}

#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
}

impl SseParser {
    pub fn push_chunk(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();

        loop {
            let sep = find_separator(&self.buffer);
            let Some((split_at, sep_len)) = sep else {
                break;
            };
            let block = self.buffer[..split_at].to_string();
            self.buffer.drain(..split_at + sep_len);
            if let Some(event) = decode_sse_event(&block) {
                events.push(event);
            }
        }

        events
    }
}

#[derive(Clone)]
pub struct SseTransportClient {
    client: reqwest::Client,
}

impl Default for SseTransportClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl SseTransportClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub async fn open_stream(
        &self,
        endpoint: &AdapterEndpoint,
        channel_size: usize,
        max_payload_size: Option<usize>,
    ) -> Result<mpsc::Receiver<SseEvent>, AdapterError> {
        let mut request = self
            .client
            .get(endpoint.url.clone())
            .timeout(endpoint.timeout());
        for (key, value) in &endpoint.headers {
            request = request.header(key, value);
        }
        if let Some(token) = &endpoint.token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|err| AdapterError::Sse(format!("sse request failed: {}", err)))?;
        if !response.status().is_success() {
            return Err(AdapterError::Sse(format!(
                "sse endpoint returned status {}",
                response.status()
            )));
        }

        let (tx, rx) = mpsc::channel(channel_size.max(1));
        tokio::spawn(async move {
            let mut parser = SseParser::default();
            let mut stream = response.bytes_stream();
            while let Some(next) = stream.next().await {
                let Ok(chunk) = next else {
                    break;
                };
                if exceeds_limit(chunk.len(), max_payload_size) {
                    break;
                }
                let text = String::from_utf8_lossy(&chunk);
                for event in parser.push_chunk(&text) {
                    if exceeds_limit(event.data.len(), max_payload_size) {
                        continue;
                    }
                    if tx.send(event).await.is_err() {
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }
}

fn exceeds_limit(payload_len: usize, max_payload_size: Option<usize>) -> bool {
    max_payload_size.is_some_and(|limit| payload_len > limit)
}

fn find_separator(input: &str) -> Option<(usize, usize)> {
    if let Some(index) = input.find("\r\n\r\n") {
        return Some((index, 4));
    }
    if let Some(index) = input.find("\n\n") {
        return Some((index, 2));
    }
    None
}
