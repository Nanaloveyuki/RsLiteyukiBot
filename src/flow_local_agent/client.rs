use std::borrow::Cow;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use super::protocol::{
    FlowLocalAgentClientMessage, FlowLocalAgentCloseCode, FlowLocalAgentRequest,
    FlowLocalAgentServerMessage, FlowLocalAgentToolResponse,
};
use super::state::FlowLocalAgentRuntimeState;
use super::tools::FlowLocalAgentToolExecutor;
use crate::app_config::FlowLocalAgentRuntimeConfig;
use liteyukibot_core::{LogLevel, emit_console_log};

const FLOW_LOCAL_AGENT_RECONNECT_DELAY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct FlowLocalAgentClient {
    state: FlowLocalAgentRuntimeState,
    runtime_config: Option<FlowLocalAgentRuntimeConfig>,
    tool_executor: Option<FlowLocalAgentToolExecutor>,
}

impl FlowLocalAgentClient {
    pub fn new(state: FlowLocalAgentRuntimeState) -> Self {
        Self {
            state,
            runtime_config: None,
            tool_executor: None,
        }
    }

    pub(crate) fn with_runtime_config(
        runtime_config: FlowLocalAgentRuntimeConfig,
        state: FlowLocalAgentRuntimeState,
    ) -> Self {
        let tool_executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config);
        Self {
            state,
            runtime_config: Some(runtime_config),
            tool_executor: Some(tool_executor),
        }
    }

    pub fn state(&self) -> &FlowLocalAgentRuntimeState {
        &self.state
    }

    pub(crate) fn spawn_background(&self) {
        let client = self.clone();
        tokio::spawn(async move {
            if let Err(err) = client.run().await {
                emit_console_log(
                    LogLevel::Warn,
                    "flow.local_agent",
                    format!("flow local agent stopped: {err}"),
                );
            }
        });
    }

    pub async fn run(&self) -> Result<(), String> {
        let runtime_config = self
            .runtime_config
            .clone()
            .ok_or_else(|| "flow local agent runtime config is not wired yet".to_string())?;

        if !runtime_config.enabled {
            self.state.mark_disconnected(false, None);
            emit_console_log(
                LogLevel::Info,
                "flow.local_agent",
                "flow local agent is disabled; skipping connection loop",
            );
            return Ok(());
        }
        if !runtime_config.auto_connect {
            self.state.mark_disconnected(false, None);
            emit_console_log(
                LogLevel::Info,
                "flow.local_agent",
                "flow local agent auto_connect is disabled; skipping connection loop",
            );
            return Ok(());
        }

        let token = runtime_config
            .token
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "flow local agent token is required when enabled".to_string())?;
        let url = build_websocket_url(&runtime_config, token)?;
        let device_label = display_device_label(&runtime_config);

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent",
            format!(
                "flow local agent starting (device={}, server={})",
                device_label,
                sanitize_server_for_log(url.as_str())
            ),
        );

        loop {
            emit_console_log(
                LogLevel::Debug,
                "flow.local_agent",
                format!(
                    "attempting outbound websocket connection (device={}, server={})",
                    device_label,
                    sanitize_server_for_log(url.as_str())
                ),
            );
            match connect_async(url.as_str()).await {
                Ok((stream, _)) => {
                    self.state.mark_connected();
                    emit_console_log(
                        LogLevel::Info,
                        "flow.local_agent",
                        format!(
                            "flow local agent connected (device={}, server={})",
                            device_label,
                            sanitize_server_for_log(url.as_str())
                        ),
                    );

                    let (mut write, mut read) = stream.split();
                    let connection_result = async {
                        while let Some(message) = read.next().await {
                            let message = message
                                .map_err(|err| format!("flow local agent read failed: {err}"))?;
                            match message {
                                Message::Text(text) => {
                                    self.handle_text_message(&mut write, text.as_ref()).await?;
                                }
                                Message::Binary(payload) => {
                                    if let Ok(text) = String::from_utf8(payload.to_vec()) {
                                        self.handle_text_message(&mut write, text.as_str()).await?;
                                    }
                                }
                                Message::Ping(payload) => {
                                    write
                                        .send(Message::Pong(payload))
                                        .await
                                        .map_err(|err| format!("flow local agent pong failed: {err}"))?;
                                }
                                Message::Pong(_) => {}
                                Message::Close(frame) => {
                                    let code = frame.as_ref().map(|value| value.code.into()).unwrap_or(1000);
                                    let reason = frame
                                        .as_ref()
                                        .map(|value| value.reason.to_string())
                                        .unwrap_or_else(|| "connection closed".to_string());
                                    let reconnect_allowed =
                                        FlowLocalAgentCloseCode::should_reconnect(code);
                                    self.state.mark_disconnected(
                                        reconnect_allowed,
                                        Some(format!("close code {code}: {reason}")),
                                    );
                                    emit_console_log(
                                        if reconnect_allowed {
                                            LogLevel::Warn
                                        } else {
                                            LogLevel::Info
                                        },
                                        "flow.local_agent",
                                        format!(
                                            "flow local agent disconnected (device={}, code={}, reconnect={})",
                                            device_label, code, reconnect_allowed
                                        ),
                                    );
                                    return Ok(reconnect_allowed);
                                }
                                Message::Frame(_) => {}
                            }
                        }
                        Err("flow local agent websocket ended without close frame".to_string())
                    }
                    .await;

                    match connection_result {
                        Ok(true) => {
                            emit_console_log(
                                LogLevel::Debug,
                                "flow.local_agent",
                                format!(
                                    "flow local agent scheduling reconnect in {}s (device={})",
                                    FLOW_LOCAL_AGENT_RECONNECT_DELAY.as_secs(),
                                    device_label
                                ),
                            );
                            sleep(FLOW_LOCAL_AGENT_RECONNECT_DELAY).await;
                        }
                        Ok(false) => return Ok(()),
                        Err(err) => {
                            self.state.mark_disconnected(true, Some(err.clone()));
                            emit_console_log(
                                LogLevel::Warn,
                                "flow.local_agent",
                                format!(
                                    "flow local agent connection error (device={}): {err}",
                                    device_label
                                ),
                            );
                            emit_console_log(
                                LogLevel::Debug,
                                "flow.local_agent",
                                format!(
                                    "flow local agent retrying in {}s after connection error (device={})",
                                    FLOW_LOCAL_AGENT_RECONNECT_DELAY.as_secs(),
                                    device_label
                                ),
                            );
                            sleep(FLOW_LOCAL_AGENT_RECONNECT_DELAY).await;
                        }
                    }
                }
                Err(err) => {
                    let message = format!("flow local agent connect failed: {err}");
                    self.state.mark_disconnected(true, Some(message.clone()));
                    emit_console_log(
                        LogLevel::Warn,
                        "flow.local_agent",
                        format!(
                            "flow local agent connect failed (device={}, server={}): {err}",
                            device_label,
                            sanitize_server_for_log(url.as_str())
                        ),
                    );
                    emit_console_log(
                        LogLevel::Debug,
                        "flow.local_agent",
                        format!(
                            "flow local agent retrying in {}s after connect failure (device={})",
                            FLOW_LOCAL_AGENT_RECONNECT_DELAY.as_secs(),
                            device_label
                        ),
                    );
                    sleep(FLOW_LOCAL_AGENT_RECONNECT_DELAY).await;
                }
            }
        }
    }

    async fn handle_text_message<S>(&self, write: &mut S, text: &str) -> Result<(), String>
    where
        S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    {
        let message = serde_json::from_str::<FlowLocalAgentServerMessage>(text)
            .map_err(|err| format!("invalid flow local agent message: {err}"))?;
        match message {
            FlowLocalAgentServerMessage::Ping(_) => {
                emit_console_log(
                    LogLevel::Debug,
                    "flow.local_agent",
                    "flow local agent received ping; replying with pong",
                );
                let payload = serde_json::to_string(&FlowLocalAgentClientMessage::Pong)
                    .map_err(|err| format!("failed to serialize pong: {err}"))?;
                write
                    .send(Message::Text(payload.into()))
                    .await
                    .map_err(|err| format!("flow local agent send pong failed: {err}"))?;
            }
            FlowLocalAgentServerMessage::ConfirmResponse(response) => {
                emit_console_log(
                    LogLevel::Info,
                    "flow.local_agent",
                    format!(
                        "flow local agent received confirm response (id={}, approved={}, always={})",
                        response.id, response.approved, response.always
                    ),
                );
            }
            FlowLocalAgentServerMessage::Request(request) => {
                self.handle_request(write, request).await?;
            }
        }
        Ok(())
    }

    async fn handle_request<S>(
        &self,
        write: &mut S,
        request: FlowLocalAgentRequest,
    ) -> Result<(), String>
    where
        S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    {
        let request_id = request.id.clone();
        emit_console_log(
            LogLevel::Info,
            "flow.local_agent",
            format!(
                "flow local agent received remote request (id={}, tool={})",
                request.id, request.tool
            ),
        );
        let response = match self.execute_request(request).await {
            Ok(result) => FlowLocalAgentToolResponse {
                id: request_id.clone(),
                result: Some(result),
                error: None,
            },
            Err(error) => FlowLocalAgentToolResponse {
                id: request_id,
                result: None,
                error: Some(error),
            },
        };
        let payload = serde_json::to_string(&response)
            .map_err(|err| format!("failed to serialize tool response: {err}"))?;
        write
            .send(Message::Text(payload.into()))
            .await
            .map_err(|err| format!("flow local agent send response failed: {err}"))?;
        Ok(())
    }

    async fn execute_request(&self, request: FlowLocalAgentRequest) -> Result<String, String> {
        let executor = self
            .tool_executor
            .as_ref()
            .ok_or_else(|| "flow local agent tool executor is not available".to_string())?;
        executor.execute(&request).await
    }
}

fn build_websocket_url(
    runtime_config: &FlowLocalAgentRuntimeConfig,
    token: &str,
) -> Result<Url, String> {
    let base_url = runtime_config
        .base_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "flow local agent base_url is required when enabled".to_string())?;
    let mut url = Url::parse(base_url)
        .map_err(|err| format!("invalid flow local agent base_url '{base_url}': {err}"))?;
    let scheme = match url.scheme() {
        "https" => "wss".to_string(),
        "http" => "ws".to_string(),
        "wss" => "wss".to_string(),
        "ws" => "ws".to_string(),
        other => {
            return Err(format!(
                "unsupported flow local agent base_url scheme '{other}'"
            ));
        }
    };
    url.set_scheme(scheme.as_str())
        .map_err(|_| "failed to normalize flow local agent websocket scheme".to_string())?;
    url.set_path("/ws/local-agent");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("token", token);
        if let Some(device_id) = runtime_config
            .device_id
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            query.append_pair("device_id", device_id);
        }
        query.append_pair("device_name", default_device_name(runtime_config).as_ref());
        query.append_pair("os", std::env::consts::OS);
        query.append_pair("version", env!("CARGO_PKG_VERSION"));
    }
    Ok(url)
}

fn default_device_name(runtime_config: &FlowLocalAgentRuntimeConfig) -> Cow<'_, str> {
    if let Some(device_name) = runtime_config
        .device_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Cow::Borrowed(device_name);
    }

    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(Cow::Owned)
        .unwrap_or_else(|| Cow::Borrowed("Liteyuki"))
}

fn display_device_label(runtime_config: &FlowLocalAgentRuntimeConfig) -> String {
    let device_name = default_device_name(runtime_config);
    match runtime_config
        .device_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(device_id) => format!("{device_name} ({device_id})"),
        None => device_name.into_owned(),
    }
}

fn sanitize_server_for_log(url: &str) -> String {
    Url::parse(url)
        .map(|mut parsed| {
            parsed.set_query(None);
            parsed.to_string()
        })
        .unwrap_or_else(|_| "<invalid-url>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_runtime_config() -> FlowLocalAgentRuntimeConfig {
        FlowLocalAgentRuntimeConfig {
            enabled: true,
            base_url: Some("https://flow.liteyuki.org/app".to_string()),
            token: Some("lys_test".to_string()),
            device_id: Some("device-1".to_string()),
            device_name: Some("Test Device".to_string()),
            auto_connect: true,
            allowed_tools: vec!["read_file".to_string()],
            workspace_root: None,
            command_timeout_ms: 30_000,
            approval_policy: "prompt".to_string(),
        }
    }

    #[test]
    fn websocket_url_uses_local_agent_path_and_redacts_from_logs() {
        let config = test_runtime_config();
        let url = build_websocket_url(&config, "lys_test").expect("url should build");
        assert_eq!(url.scheme(), "wss");
        assert_eq!(url.path(), "/ws/local-agent");
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "device_name")
                .map(|(_, value)| value.into_owned()),
            Some("Test Device".to_string())
        );

        let sanitized = sanitize_server_for_log(url.as_str());
        assert_eq!(sanitized, "wss://flow.liteyuki.org/ws/local-agent");
        assert!(!sanitized.contains("lys_test"));
    }

    #[tokio::test]
    async fn disabled_runtime_short_circuits_without_error() {
        let mut config = test_runtime_config();
        config.enabled = false;

        let state = FlowLocalAgentRuntimeState::default();
        let client = FlowLocalAgentClient::with_runtime_config(config, state.clone());
        client.run().await.expect("disabled runtime should skip");

        let snapshot = state.snapshot();
        assert!(!snapshot.connected);
        assert!(!snapshot.reconnect_allowed);
        assert!(snapshot.last_error.is_none());
    }
}
