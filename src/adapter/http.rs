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
            request = request.json(payload);
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

        response
            .json::<Value>()
            .await
            .map_err(|err| AdapterError::Serialize(format!("failed to decode json: {}", err)))
    }

    pub async fn post_packet(
        &self,
        endpoint: &AdapterEndpoint,
        packet: &AdapterPacket,
    ) -> Result<Value, AdapterError> {
        self.request_json(HttpMethod::POST, endpoint, Some(packet)).await
    }
}
