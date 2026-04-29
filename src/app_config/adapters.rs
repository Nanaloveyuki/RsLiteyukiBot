use super::access::{config_adapters, config_connect};
use super::*;

pub(crate) fn load_adapter_configs(
    app_config: &AppConfigDoc,
) -> Result<Vec<AdapterConfig>, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("LY_ADAPTERS_PATH")
        && !path.trim().is_empty()
    {
        let content = std::fs::read_to_string(PathBuf::from(path))?;
        return parse_adapter_configs_json(&content, "LY_ADAPTERS_PATH");
    }

    if let Ok(raw) = std::env::var("LY_ADAPTERS_JSON")
        && !raw.trim().is_empty()
    {
        return parse_adapter_configs_json(&raw, "LY_ADAPTERS_JSON");
    }

    let mut combined = config_adapters(app_config).cloned().unwrap_or_default();
    combined.extend(connect_to_adapter_configs(app_config));
    Ok(sanitize_adapter_configs(combined, "config"))
}

fn parse_adapter_configs_json(
    raw: &str,
    source: &str,
) -> Result<Vec<AdapterConfig>, Box<dyn std::error::Error>> {
    if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(raw) {
        return Ok(sanitize_adapter_configs(doc.adapters, source));
    }
    let list = serde_json::from_str::<Vec<AdapterConfig>>(raw)?;
    Ok(sanitize_adapter_configs(list, source))
}

pub(crate) fn connect_to_adapter_configs(doc: &AppConfigDoc) -> Vec<AdapterConfig> {
    let Some(connect) = config_connect(doc) else {
        return Vec::new();
    };

    let mut adapters = Vec::new();

    if let Some(ws) = &connect.websocket {
        let mut has_nested = false;
        if let Some(forward) = &ws.forward {
            has_nested = true;
            if forward.enabled.unwrap_or(false) {
                adapters.extend(websocket_endpoint_to_adapters(
                    "connect-ws-forward",
                    true,
                    forward,
                    ws,
                ));
            }
        }
        if let Some(reverse) = &ws.reverse {
            has_nested = true;
            if reverse.enabled.unwrap_or(false) {
                adapters.extend(websocket_endpoint_to_adapters(
                    "connect-ws-reverse",
                    false,
                    reverse,
                    ws,
                ));
            }
        }

        if !has_nested && ws.enabled.unwrap_or(false) {
            let mode = ws.mode.as_deref().unwrap_or_default().to_ascii_lowercase();
            let resolved_mode = if mode.is_empty() {
                if ws.port.is_some() && ws.url.is_none() {
                    "reverse".to_string()
                } else {
                    "forward".to_string()
                }
            } else {
                mode
            };

            if matches!(resolved_mode.as_str(), "forward" | "both" | "all") {
                adapters.extend(websocket_root_to_adapters("connect-ws-forward", true, ws));
            }
            if matches!(resolved_mode.as_str(), "reverse" | "both" | "all") {
                adapters.extend(websocket_root_to_adapters("connect-ws-reverse", false, ws));
            }
        }
    }

    if let Some(http) = &connect.tcp_http
        && http.enabled.unwrap_or(false)
    {
        adapters.extend(http_to_adapters("connect-http", http));
    }

    if let Some(sse) = &connect.sse
        && sse.enabled.unwrap_or(false)
    {
        adapters.extend(sse_to_adapters("connect-sse", sse));
    }

    adapters
}

fn websocket_endpoint_to_adapters(
    base_id: &str,
    is_forward: bool,
    endpoint: &WebSocketEndpointSection,
    fallback: &WebSocketConnectSection,
) -> Vec<AdapterConfig> {
    use liteyukibot_core::{AdapterEndpoint, AdapterRoute, AdapterTransport};

    let mut urls =
        super::normalize_non_empty_list(endpoint.urls.as_ref().or(fallback.urls.as_ref()));
    if urls.is_empty() {
        let fallback_url = endpoint
            .url
            .clone()
            .or_else(|| fallback.url.clone())
            .or_else(|| {
                super::build_url(
                    "ws",
                    endpoint
                        .host
                        .as_deref()
                        .or(fallback.host.as_deref())
                        .unwrap_or(if is_forward { "127.0.0.1" } else { "0.0.0.0" }),
                    endpoint.port.or(fallback.port),
                    endpoint
                        .path
                        .as_deref()
                        .or(fallback.path.as_deref())
                        .unwrap_or(if is_forward { "/ws" } else { "/" }),
                )
            });
        if let Some(url) = super::normalize_non_empty(fallback_url) {
            urls.push(url);
        }
    }

    let total = urls.len();
    urls.into_iter()
        .enumerate()
        .map(|(idx, url)| AdapterConfig {
            id: super::indexed_adapter_id(base_id, idx, total),
            enabled: true,
            transport: if is_forward {
                AdapterTransport::WebSocketForward
            } else {
                AdapterTransport::WebSocketReverse
            },
            endpoint: AdapterEndpoint {
                url,
                headers: endpoint
                    .headers
                    .clone()
                    .or_else(|| fallback.headers.clone())
                    .unwrap_or_default(),
                token: endpoint.token.clone().or_else(|| fallback.token.clone()),
                timeout_ms: super::seconds_to_timeout_ms(
                    endpoint.timeout_seconds.or(fallback.timeout_seconds),
                ),
            },
            route: AdapterRoute {
                inbound_topic: endpoint
                    .inbound_topic
                    .clone()
                    .or_else(|| fallback.inbound_topic.clone())
                    .unwrap_or_else(|| "adapter.inbound".to_string()),
                outbound_topic: endpoint
                    .outbound_topic
                    .clone()
                    .or_else(|| fallback.outbound_topic.clone())
                    .unwrap_or_else(|| "adapter.outbound".to_string()),
            },
            queue_capacity: endpoint
                .queue_capacity
                .or(fallback.queue_capacity)
                .unwrap_or(256),
            max_payload_size: endpoint.max_payload_size.or(fallback.max_payload_size),
            max_connections: endpoint.max_connections.or(fallback.max_connections),
        })
        .collect()
}

fn websocket_root_to_adapters(
    base_id: &str,
    is_forward: bool,
    ws: &WebSocketConnectSection,
) -> Vec<AdapterConfig> {
    let endpoint = WebSocketEndpointSection {
        enabled: Some(true),
        url: ws.url.clone(),
        urls: ws.urls.clone(),
        host: ws.host.clone(),
        port: ws.port,
        path: ws.path.clone(),
        headers: ws.headers.clone(),
        token: ws.token.clone(),
        timeout_seconds: ws.timeout_seconds,
        queue_capacity: ws.queue_capacity,
        max_payload_size: ws.max_payload_size,
        max_connections: ws.max_connections,
        inbound_topic: ws.inbound_topic.clone(),
        outbound_topic: ws.outbound_topic.clone(),
    };
    websocket_endpoint_to_adapters(base_id, is_forward, &endpoint, ws)
}

fn http_to_adapters(base_id: &str, section: &HttpConnectSection) -> Vec<AdapterConfig> {
    use liteyukibot_core::{AdapterEndpoint, AdapterRoute, AdapterTransport};

    let mut urls = super::normalize_non_empty_list(section.urls.as_ref());
    if urls.is_empty() {
        let fallback_url = section.url.clone().or_else(|| {
            super::build_url(
                "http",
                section.host.as_deref().unwrap_or("127.0.0.1"),
                section.port,
                section.path.as_deref().unwrap_or("/"),
            )
        });
        urls.push(
            super::normalize_non_empty(fallback_url)
                .unwrap_or_else(|| "http://127.0.0.1:8081/".to_string()),
        );
    }

    let total = urls.len();
    urls.into_iter()
        .enumerate()
        .map(|(idx, url)| AdapterConfig {
            id: super::indexed_adapter_id(base_id, idx, total),
            enabled: true,
            transport: AdapterTransport::Http,
            endpoint: AdapterEndpoint {
                url,
                headers: section.headers.clone().unwrap_or_default(),
                token: section.token.clone(),
                timeout_ms: super::seconds_to_timeout_ms(section.timeout_seconds),
            },
            route: AdapterRoute {
                inbound_topic: section
                    .inbound_topic
                    .clone()
                    .unwrap_or_else(|| "adapter.inbound".to_string()),
                outbound_topic: section
                    .outbound_topic
                    .clone()
                    .unwrap_or_else(|| "adapter.outbound".to_string()),
            },
            queue_capacity: section.queue_capacity.unwrap_or(256),
            max_payload_size: section.max_payload_size,
            max_connections: section.max_connections,
        })
        .collect()
}

fn sse_to_adapters(base_id: &str, section: &SseConnectSection) -> Vec<AdapterConfig> {
    use liteyukibot_core::{AdapterEndpoint, AdapterRoute, AdapterTransport};

    let mut urls = super::normalize_non_empty_list(section.urls.as_ref());
    if urls.is_empty() {
        let fallback_url = section.url.clone().or_else(|| {
            super::build_url(
                "http",
                section.host.as_deref().unwrap_or("127.0.0.1"),
                section.port,
                section.path.as_deref().unwrap_or("/sse"),
            )
        });
        urls.push(
            super::normalize_non_empty(fallback_url)
                .unwrap_or_else(|| "http://127.0.0.1:8082/sse".to_string()),
        );
    }

    let total = urls.len();
    urls.into_iter()
        .enumerate()
        .map(|(idx, url)| AdapterConfig {
            id: super::indexed_adapter_id(base_id, idx, total),
            enabled: true,
            transport: AdapterTransport::Sse,
            endpoint: AdapterEndpoint {
                url,
                headers: section.headers.clone().unwrap_or_default(),
                token: section.token.clone(),
                timeout_ms: super::seconds_to_timeout_ms(section.timeout_seconds),
            },
            route: AdapterRoute {
                inbound_topic: section
                    .inbound_topic
                    .clone()
                    .unwrap_or_else(|| "adapter.inbound".to_string()),
                outbound_topic: section
                    .outbound_topic
                    .clone()
                    .unwrap_or_else(|| "adapter.outbound".to_string()),
            },
            queue_capacity: section.queue_capacity.unwrap_or(256),
            max_payload_size: section.max_payload_size,
            max_connections: section.max_connections,
        })
        .collect()
}
