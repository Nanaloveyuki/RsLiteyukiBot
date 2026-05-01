mod assets;
mod auth;
mod capability_api;
mod config;
mod file_api;
mod http;
mod llm_api;
mod log_api;
mod mirror_api;
mod mirror_support;
mod plugin_api;
mod plugin_install;
mod plugin_pages;
mod plugin_runtime;
mod plugin_store;
mod realtime;
mod release_api;
mod router;
mod skill_import;
mod system_api;
mod terminal;
mod upstream;
mod webui_config_api;
mod workspace;

pub use self::assets::{WebHostAsset, WebHostAssets};
use self::auth::*;
use self::config::*;
use self::http::*;
use self::plugin_runtime::*;
use self::realtime::*;
use self::terminal::*;
use self::workspace::*;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::net::{
    IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener, TcpStream as StdTcpStream,
};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use portable_pty::{CommandBuilder as PtyCommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

use super::ui::{NapCatConfig, NapCatWebUIConfig, OneBotConfig};
use crate::app_config::{
    load_app_config_with_warnings, resolve_app_config_path, resolve_disabled_plugins,
};
use crate::app_host::{AppHostPluginCatalogSnapshot, AppHostSnapshot, EmbeddedAppHost};
use crate::config_edit::persist_disabled_plugins;
use crate::flow_local_agent::FlowLocalAgentRuntimeState;
use crate::i18n::current_snapshot as current_i18n_snapshot;
use crate::observability::{BufferedLogEntry, recent_buffered_logs};
use crate::plugin::source_adapter::{
    descriptor_adapter_family, descriptor_compat_kind, descriptor_compat_level,
    descriptor_family_value, descriptor_source_family, descriptor_source_kind,
    discover_plugin_manifests_in_dirs,
};
use crate::runtime_support::{resolve_builtin_plugin_dirs, resolve_local_plugin_dir};
use crate::{LogLevel, PluginLoadState, PluginManifestLoader, PluginSdk, emit_console_log};
const DEFAULT_HTTP_PORT: u16 = 14500;
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const REQUEST_READ_CHUNK_BYTES: usize = 2048;
const HEALTH_ROUTE: &str = "/api/health";
const LOGS_ROUTE: &str = "/api/logs";
const I18N_ROUTE: &str = "/api/i18n";
const LOGS_ROUTE_LIMIT: usize = 200;
const DEV_FRONTEND_PROBE_TIMEOUT: Duration = Duration::from_millis(150);
const WEBUI_STATE_DIR: &str = "config/webui";
const ONEBOT_CONFIG_FILE: &str = "config/webui/onebot-v11.json";
const NAPCAT_CONFIG_FILE: &str = "config/webui/napcat.json";
const NAPCAT_UIN_CONFIG_FILE: &str = "config/webui/napcat-uin.json";
const WEBUI_SERVER_CONFIG_FILE: &str = "config/webui/server.json";
const WEBUI_APPEARANCE_CONFIG_FILE: &str = "config/webui/appearance.json";
const THEME_CONFIG_FILE: &str = "config/webui/theme.json";
const MIRROR_CONFIG_FILE: &str = "config/webui/mirrors.json";
const SSL_CERT_FILE: &str = "config/webui/cert.pem";
const SSL_KEY_FILE: &str = "config/webui/key.pem";
const CUSTOM_FONT_FILE: &str = "config/webui/fonts/CustomFont.woff";
const PUBLIC_FONT_DIR: &str = "frontend/public/fonts";
const WORKSPACE_FILE_DOWNLOAD_NAME: &str = "workspace.txt";
const SOURCE_ADAPTER_MANIFEST_DIR: &str = "manifests";
const SOURCE_ADAPTER_OVERRIDE_SUFFIX: &str = ".override.json";
const TERMINAL_WS_PATH: &str = "/api/ws/terminal";
const TERMINAL_DEFAULT_COLS: u16 = 80;
const TERMINAL_DEFAULT_ROWS: u16 = 24;
const TERMINAL_OUTPUT_CHANNEL_CAPACITY: usize = 256;
const TERMINAL_STREAM_BUFFER_BYTES: usize = 4096;
const TERMINAL_HISTORY_LINES: usize = 3;
const TERMINAL_WS_BATCH_INTERVAL: Duration = Duration::from_millis(40);
const TERMINAL_STATE_IDLE: u8 = 0;
const TERMINAL_STATE_STARTING: u8 = 1;
const TERMINAL_STATE_RUNNING: u8 = 2;
const TERMINAL_STATE_CLOSED: u8 = 3;
const LOG_REALTIME_ROUTE: &str = "/api/Log/GetLogRealTime";
const SYSTEM_STATUS_REALTIME_ROUTE: &str = "/api/base/GetSysStatusRealTime";
const REALTIME_LOG_STREAM_LIMIT: usize = 400;
const REALTIME_STREAM_INTERVAL: Duration = Duration::from_secs(1);
const SSE_KEEPALIVE_TICKS: usize = 15;

fn run_async_for_web_host<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("web host helper runtime should build")
            .block_on(future)
    }
}

pub type WebHostSnapshotProvider = Arc<dyn Fn() -> AppHostSnapshot + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebHostConfig {
    pub bind_ip: IpAddr,
    pub browser_ip: IpAddr,
    pub port: u16,
    pub dev_frontend: Option<WebHostDevServer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebHostDevServer {
    pub probe_addr: SocketAddr,
    pub public_port: u16,
}

impl Default for WebHostConfig {
    fn default() -> Self {
        Self {
            bind_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: DEFAULT_HTTP_PORT,
            dev_frontend: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WebHostHealthPayload {
    pub bind: String,
    pub desktop_url: String,
    pub external_url_hint: String,
    pub runtime: AppHostSnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebHostLogsPayload {
    pub entries: Vec<BufferedLogEntry>,
}

#[derive(Clone)]
pub struct WebHostService {
    bind_addr: SocketAddr,
    browser_ip: IpAddr,
    dev_frontend: Option<WebHostDevServer>,
    snapshot_provider: WebHostSnapshotProvider,
    runtime_host: Option<EmbeddedAppHost>,
    assets: Arc<WebHostAssets>,
    terminal_state: Arc<WebTerminalState>,
    flow_local_agent_state: Option<FlowLocalAgentRuntimeState>,
    auth: WebUiAuthManager,
}

impl WebHostService {
    pub fn bind(
        config: WebHostConfig,
        snapshot_provider: WebHostSnapshotProvider,
        assets: WebHostAssets,
    ) -> Result<(Self, TcpListener), String> {
        let auth = WebUiAuthManager::load_or_init_default()?;
        if auth.status().password_configured {
            emit_console_log(
                LogLevel::Info,
                "web.host.auth",
                format!(
                    "webui password login enabled (store: {})",
                    auth.storage_path()
                        .unwrap_or_else(|| PathBuf::from("<memory>"))
                        .display()
                ),
            );
        } else {
            emit_console_log(
                LogLevel::Info,
                "web.host.auth",
                format!(
                    "webui bootstrap login token: {} (valid until a password is configured)",
                    auth.bootstrap_login_token()
                ),
            );
        }

        let bind_addr = SocketAddr::new(config.bind_ip, config.port);
        let listener = StdTcpListener::bind(bind_addr)
            .map_err(|err| format!("failed to bind shared HTTP host on {bind_addr}: {err}"))?;
        listener.set_nonblocking(true).map_err(|err| {
            format!("failed to switch shared HTTP host listener to nonblocking mode: {err}")
        })?;
        let bind_addr = listener
            .local_addr()
            .map_err(|err| format!("failed to read shared HTTP host bind address: {err}"))?;
        let listener = TcpListener::from_std(listener)
            .map_err(|err| format!("failed to convert shared HTTP host listener: {err}"))?;

        Ok((
            Self {
                bind_addr,
                browser_ip: config.browser_ip,
                dev_frontend: config.dev_frontend,
                snapshot_provider,
                runtime_host: None,
                assets: Arc::new(assets),
                terminal_state: Arc::new(WebTerminalState::default()),
                flow_local_agent_state: None,
                auth,
            },
            listener,
        ))
    }

    pub fn bind_default(
        snapshot_provider: WebHostSnapshotProvider,
        assets: WebHostAssets,
    ) -> Result<(Self, TcpListener), String> {
        Self::bind(WebHostConfig::default(), snapshot_provider, assets)
    }

    pub fn with_runtime_host(mut self, runtime_host: EmbeddedAppHost) -> Self {
        self.runtime_host = Some(runtime_host);
        self
    }

    pub fn with_flow_local_agent_state(
        mut self,
        flow_local_agent_state: FlowLocalAgentRuntimeState,
    ) -> Self {
        self.flow_local_agent_state = Some(flow_local_agent_state);
        self
    }

    pub fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    pub fn desktop_url(&self) -> String {
        format!("http://{}:{}/", self.browser_ip, self.bind_addr.port())
    }

    pub fn external_url_hint(&self) -> String {
        format!("http://<host-ip>:{}/", self.bind_addr.port())
    }

    pub fn desktop_webui_url(&self) -> String {
        format!("{}webui/", self.desktop_url())
    }

    pub fn external_webui_url_hint(&self) -> String {
        format!("{}webui/", self.external_url_hint())
    }

    pub fn local_token(&self) -> String {
        self.auth.local_session_token()
    }

    pub fn health(&self) -> WebHostHealthPayload {
        WebHostHealthPayload {
            bind: self.bind_addr.to_string(),
            desktop_url: self.desktop_url(),
            external_url_hint: self.external_url_hint(),
            runtime: (self.snapshot_provider)(),
        }
    }

    pub async fn serve(self, listener: TcpListener) -> io::Result<()> {
        loop {
            let (socket, peer_addr) = listener.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(err) = server.handle_connection(socket, peer_addr).await {
                    let level = if is_expected_client_disconnect(&err) {
                        LogLevel::Debug
                    } else {
                        LogLevel::Warn
                    };
                    emit_console_log(
                        level,
                        "web.host",
                        format!("failed to serve shared HTTP request: {err}"),
                    );
                }
            });
        }
    }

    async fn handle_connection(
        &self,
        mut socket: TcpStream,
        peer_addr: SocketAddr,
    ) -> io::Result<()> {
        if let Some(request_line) = peek_http_request_line(&mut socket).await?
            && let Some((method, raw_path)) = parse_request_line(&request_line)
            && method.eq_ignore_ascii_case("GET")
            && raw_path.starts_with(TERMINAL_WS_PATH)
        {
            return handle_terminal_websocket(self, socket).await;
        }

        let request = read_http_request(&mut socket).await?;
        if request.is_empty() {
            return Ok(());
        }

        if let Some((method, raw_path)) = parse_request_line(&String::from_utf8_lossy(&request)) {
            let path = raw_path.split('?').next().unwrap_or(raw_path);
            if method.eq_ignore_ascii_case("GET") && path == LOG_REALTIME_ROUTE {
                if !request_is_authorized(&self.auth, request.as_slice()) {
                    let response = unauthorized_response(false);
                    socket.write_all(&response).await?;
                    return socket.shutdown().await;
                }
                return stream_realtime_logs(self, socket).await;
            }
            if method.eq_ignore_ascii_case("GET") && path == SYSTEM_STATUS_REALTIME_ROUTE {
                if !request_is_authorized(&self.auth, request.as_slice()) {
                    let response = unauthorized_response(false);
                    socket.write_all(&response).await?;
                    return socket.shutdown().await;
                }
                return stream_system_status(self, socket).await;
            }
        }

        let response = self.route_http_request(&request, peer_addr.ip());
        socket.write_all(&response).await?;
        socket.shutdown().await
    }

    fn route_http_request(&self, request: &[u8], peer_ip: IpAddr) -> Vec<u8> {
        router::route_http_request(self, request, peer_ip)
    }

    #[allow(clippy::too_many_lines)]
    fn route_napcat_api(
        &self,
        request: &[u8],
        method: &str,
        path: &str,
        raw_path: &str,
        is_head: bool,
        peer_ip: IpAddr,
    ) -> Vec<u8> {
        router::route_napcat_api(self, request, method, path, raw_path, is_head, peer_ip)
    }
}

fn is_expected_client_disconnect(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
    ) || matches!(err.raw_os_error(), Some(10053 | 10054))
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
