use reqwest::{Client, Method};
use serde::Serialize;
use serde_json::Value;

use super::{error::AdapterError, model::AdapterEndpoint, packet::AdapterPacket};

pub type HttpMethod = Method;

#[derive(Clone)]
pub struct HttpTransportClient {
    client: Client,
}

impl Default for HttpTransportClient {
    fn default() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

impl HttpTransportClient {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn request_json<T: Serialize + ?Sized>(
        &self,
        method: HttpMethod,
        endpoint: &AdapterEndpoint,
        body: Option<&T>,
        max_payload_size: Option<usize>,
    ) -> Result<Value, AdapterError> {
        let mut request = self
            .client
            .request(method, endpoint.url.clone())
            .timeout(endpoint.timeout());

        for (key, value) in &endpoint.headers {
            request = request.header(key, value);
        }

        if let Some(token) = &endpoint.token {
            request = request.bearer_auth(token);
        }

        if let Some(payload) = body {
            let payload = serde_json::to_vec(payload).map_err(|err| {
                AdapterError::Serialize(format!("failed to encode request json: {}", err))
            })?;
            enforce_payload_limit(payload.len(), max_payload_size, "http request")?;
            request = request
                .header("content-type", "application/json")
                .body(payload);
        }

        let response = request
            .send()
            .await
            .map_err(|err| AdapterError::Http(format!("http request failed: {}", err)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AdapterError::Http(format!(
                "http endpoint returned {}: {}",
                status, body
            )));
        }

        let bytes = response.bytes().await.map_err(|err| {
            AdapterError::Http(format!("failed to read http response body: {}", err))
        })?;
        enforce_payload_limit(bytes.len(), max_payload_size, "http response")?;
        serde_json::from_slice::<Value>(&bytes)
            .map_err(|err| AdapterError::Serialize(format!("failed to decode json: {}", err)))
    }

    pub async fn post_packet(
        &self,
        endpoint: &AdapterEndpoint,
        packet: &AdapterPacket,
        max_payload_size: Option<usize>,
    ) -> Result<Value, AdapterError> {
        if let Some(onebot_body) = onebot_v11_outbound_payload(packet) {
            return self
                .request_json(
                    HttpMethod::POST,
                    endpoint,
                    Some(&onebot_body),
                    max_payload_size,
                )
                .await;
        }
        self.request_json(HttpMethod::POST, endpoint, Some(packet), max_payload_size)
            .await
    }
}

fn onebot_v11_outbound_payload(packet: &AdapterPacket) -> Option<Value> {
    let object = packet.payload.as_object()?;
    if object.get("action").and_then(Value::as_str).is_some() {
        return Some(packet.payload.clone());
    }
    None
}

fn enforce_payload_limit(
    payload_len: usize,
    max_payload_size: Option<usize>,
    direction: &str,
) -> Result<(), AdapterError> {
    if let Some(limit) = max_payload_size
        && payload_len > limit
    {
        return Err(AdapterError::Http(format!(
            "{} payload exceeded limit: {} > {} bytes",
            direction, payload_len, limit
        )));
    }
    Ok(())
}
