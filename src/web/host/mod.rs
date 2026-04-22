mod config;
mod http;
mod terminal;

use self::config::*;
use self::http::*;
use self::terminal::*;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::net::{
    IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener, TcpStream as StdTcpStream,
};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU16, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder as PtyCommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use sysinfo::System;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::{MissedTickBehavior, sleep};
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

use crate::app_config::{load_app_config_with_warnings, resolve_app_config_path, resolve_disabled_plugins};
use crate::app_host::AppHostSnapshot;
use crate::config_edit::persist_disabled_plugins;
use crate::i18n::current_snapshot as current_i18n_snapshot;
use crate::observability::{BufferedLogEntry, recent_buffered_logs};
use crate::runtime_support::resolve_builtin_plugin_dirs;
use super::ui::{NapCatConfig, NapCatWebUIConfig, OneBotConfig};
use crate::{LogLevel, PluginManifestLoader, emit_console_log};

const DEFAULT_HTTP_PORT: u16 = 14500;
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const REQUEST_READ_CHUNK_BYTES: usize = 2048;
const HEALTH_ROUTE: &str = "/api/health";
const LOGS_ROUTE: &str = "/api/logs";
const I18N_ROUTE: &str = "/api/i18n";
const LOGS_ROUTE_LIMIT: usize = 200;
const DEV_FRONTEND_PROBE_TIMEOUT: Duration = Duration::from_millis(150);
/// The fixed credential returned by `/api/auth/login` and `/api/auth/local-token`.
/// Using a constant keeps the stub simple; a real implementation would use a
/// cryptographically random value generated at startup.
const LOCAL_AUTO_TOKEN: &str = "rsliteyukibot-local-token";
const WEBUI_STATE_DIR: &str = "config/webui";
const ONEBOT_CONFIG_FILE: &str = "config/webui/onebot-v11.json";
const NAPCAT_CONFIG_FILE: &str = "config/webui/napcat.json";
const NAPCAT_UIN_CONFIG_FILE: &str = "config/webui/napcat-uin.json";
const WEBUI_SERVER_CONFIG_FILE: &str = "config/webui/server.json";
const THEME_CONFIG_FILE: &str = "config/webui/theme.json";
const SSL_CERT_FILE: &str = "config/webui/cert.pem";
const SSL_KEY_FILE: &str = "config/webui/key.pem";
const CUSTOM_FONT_FILE: &str = "config/webui/fonts/CustomFont.woff";
const PUBLIC_FONT_DIR: &str = "frontend/public/fonts";
const WORKSPACE_FILE_DOWNLOAD_NAME: &str = "workspace.txt";
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

#[derive(Debug, Clone, Serialize)]
struct WorkspaceFileInfo {
    name: String,
    #[serde(rename = "isDirectory")]
    is_directory: bool,
    size: u64,
    mtime: String,
}

fn round_metric(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

fn bytes_to_mebibytes(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

fn arch_label() -> String {
    format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH)
}

fn current_cpu_profile() -> (String, usize, f32) {
    let mut system = System::new();
    system.refresh_cpu_all();

    let cpus = system.cpus();
    let model = cpus
        .iter()
        .find_map(|cpu| {
            let brand = cpu.brand().trim();
            (!brand.is_empty()).then(|| brand.to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string());
    let detected_cores = cpus.len();
    let fallback_cores = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1);
    let core_count = detected_cores.max(fallback_cores);
    let speed_ghz = cpus
        .iter()
        .find_map(|cpu| {
            let frequency_mhz = cpu.frequency();
            (frequency_mhz > 0).then_some(frequency_mhz as f32 / 1000.0)
        })
        .map(round_metric)
        .unwrap_or(0.0);

    (model, core_count, speed_ghz)
}

fn napcat_system_status(snapshot: &AppHostSnapshot) -> serde_json::Value {
    let (cpu_model, cpu_core_count, cpu_speed_ghz) = current_cpu_profile();

    serde_json::json!({
        "cpu": {
            "core": cpu_core_count,
            "model": cpu_model,
            "speed": cpu_speed_ghz,
            "usage": {
                "system": round_metric(snapshot.resource_usage.cpu.system_percent),
                "qq": round_metric(snapshot.resource_usage.cpu.process_percent)
            }
        },
        "memory": {
            "total": bytes_to_mebibytes(snapshot.resource_usage.memory.total_bytes),
            "usage": {
                "system": bytes_to_mebibytes(snapshot.resource_usage.memory.used_bytes),
                "qq": bytes_to_mebibytes(snapshot.resource_usage.memory.process_bytes)
            }
        },
        "arch": arch_label()
    })
}

fn workspace_root() -> PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn sanitize_workspace_relative_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_start_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }

    let mut output = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(segment) => output.push(segment),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("path escapes workspace root".to_string());
            }
        }
    }
    Ok(output)
}

fn resolve_workspace_path(raw: &str) -> Result<PathBuf, String> {
    Ok(workspace_root().join(sanitize_workspace_relative_path(raw)?.as_path()))
}

fn ensure_path_within_workspace(path: &Path) -> Result<(), String> {
    let root = workspace_root();
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if canonical.starts_with(&root) || path.starts_with(&root) {
        Ok(())
    } else {
        Err("path escapes workspace root".to_string())
    }
}

fn parent_or_self(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
}

fn build_workspace_file_info(path: &Path) -> Result<WorkspaceFileInfo, String> {
    let metadata =
        fs::metadata(path).map_err(|err| format!("failed to stat {}: {err}", path.display()))?;
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let modified: DateTime<Utc> = modified.into();

    Ok(WorkspaceFileInfo {
        name: path
            .file_name()
            .and_then(|segment| segment.to_str())
            .unwrap_or_default()
            .to_string(),
        is_directory: metadata.is_dir(),
        size: if metadata.is_file() { metadata.len() } else { 0 },
        mtime: modified.to_rfc3339(),
    })
}

fn format_log_history(limit: usize) -> String {
    recent_buffered_logs(limit)
        .into_iter()
        .map(|entry| entry.line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn localized_text(raw: &str) -> String {
    let snapshot = current_i18n_snapshot();
    snapshot
        .messages
        .get(raw)
        .cloned()
        .unwrap_or_else(|| raw.to_string())
}

fn discover_plugins() -> Vec<Value> {
    let (doc, _) = load_app_config_with_warnings(false);
    let disabled = resolve_disabled_plugins(&doc);
    let plugin_dirs = resolve_builtin_plugin_dirs();
    let Ok(manifests) = PluginManifestLoader::discover_in_dirs(plugin_dirs.iter()) else {
        return Vec::new();
    };

    manifests
        .into_iter()
        .map(|manifest| {
            let id = manifest.descriptor.metadata.id.clone();
            let description = localized_text(manifest.descriptor.metadata.description.as_str());
            let name = localized_text(manifest.descriptor.metadata.name.as_str());
            let status = if disabled.iter().any(|entry| entry == &id) {
                "disabled"
            } else {
                "active"
            };
            Value::Object(Map::from_iter([
                ("name".to_string(), Value::String(name)),
                ("id".to_string(), Value::String(id.clone())),
                (
                    "version".to_string(),
                    Value::String(
                        manifest
                            .descriptor
                            .metadata
                            .extra
                            .get("version")
                            .and_then(Value::as_str)
                            .unwrap_or("builtin")
                            .to_string(),
                    ),
                ),
                ("description".to_string(), Value::String(description)),
                (
                    "author".to_string(),
                    Value::String(manifest.descriptor.metadata.author),
                ),
                ("status".to_string(), Value::String(status.to_string())),
                ("hasConfig".to_string(), Value::Bool(false)),
                ("hasPages".to_string(), Value::Bool(false)),
                (
                    "homepage".to_string(),
                    Value::String(manifest.descriptor.metadata.homepage),
                ),
            ]))
        })
        .collect()
}

fn update_disabled_plugins(plugin_id: &str, enable: bool) -> Result<(), String> {
    let Some(config_path) = resolve_app_config_path() else {
        return Err("app config path not found".to_string());
    };
    let (doc, _) = load_app_config_with_warnings(false);
    let mut disabled = resolve_disabled_plugins(&doc);
    if enable {
        disabled.retain(|entry| entry != plugin_id);
    } else if !disabled.iter().any(|entry| entry == plugin_id) {
        disabled.push(plugin_id.to_string());
    }
    persist_disabled_plugins(config_path.as_path(), &disabled)
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

#[derive(Debug, Clone)]
pub struct WebHostAsset {
    content_type: String,
    body: Arc<[u8]>,
}

impl WebHostAsset {
    pub fn binary(content_type: impl Into<String>, body: impl AsRef<[u8]>) -> Self {
        Self {
            content_type: content_type.into(),
            body: Arc::<[u8]>::from(body.as_ref().to_vec()),
        }
    }

    pub fn text(content_type: impl Into<String>, body: impl AsRef<str>) -> Self {
        Self::binary(content_type, body.as_ref().as_bytes())
    }

    pub fn content_type(&self) -> &str {
        self.content_type.as_str()
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

#[derive(Debug, Clone)]
pub struct WebHostAssets {
    index: WebHostAsset,
    static_assets: HashMap<String, WebHostAsset>,
    asset_dir: Option<WebHostAssetDirectory>,
}

#[derive(Debug, Clone)]
struct WebHostAssetDirectory {
    root: PathBuf,
    spa_fallback_to_index: bool,
}

impl WebHostAssets {
    pub fn new(index: WebHostAsset) -> Self {
        Self {
            index,
            static_assets: HashMap::new(),
            asset_dir: None,
        }
    }

    pub fn with_asset(mut self, path: impl Into<String>, asset: WebHostAsset) -> Self {
        self.insert_asset(path, asset);
        self
    }

    pub fn insert_asset(&mut self, path: impl Into<String>, asset: WebHostAsset) {
        self.static_assets.insert(normalize_asset_path(path), asset);
    }

    pub fn with_asset_directory(mut self, root: impl Into<PathBuf>) -> Self {
        self.asset_dir = Some(WebHostAssetDirectory {
            root: root.into(),
            spa_fallback_to_index: true,
        });
        self
    }

    fn asset_for_path(&self, path: &str) -> Option<WebHostAsset> {
        if path == "/" {
            return self
                .asset_dir
                .as_ref()
                .and_then(load_directory_index_asset)
                .or_else(|| Some(self.index.clone()));
        }

        if let Some(asset) = self.static_assets.get(path) {
            return Some(asset.clone());
        }

        if let Some(directory) = &self.asset_dir {
            if let Some(asset) = load_directory_asset_for_request(directory, path) {
                return Some(asset);
            }

            if directory.spa_fallback_to_index && should_fallback_to_index(path) {
                return load_directory_index_asset(directory).or_else(|| Some(self.index.clone()));
            }
        }

        None
    }

    fn static_asset_for_path(&self, path: &str) -> Option<WebHostAsset> {
        self.static_assets.get(path).cloned()
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
    assets: Arc<WebHostAssets>,
    terminal_state: Arc<WebTerminalState>,
}

impl WebHostService {
    pub fn bind(
        config: WebHostConfig,
        snapshot_provider: WebHostSnapshotProvider,
        assets: WebHostAssets,
    ) -> Result<(Self, TcpListener), String> {
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
                assets: Arc::new(assets),
                terminal_state: Arc::new(WebTerminalState::default()),
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

    pub fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    pub fn desktop_url(&self) -> String {
        format!("http://{}:{}/", self.browser_ip, self.bind_addr.port())
    }

    pub fn external_url_hint(&self) -> String {
        format!("http://<host-ip>:{}/", self.bind_addr.port())
    }

    /// Returns the local auto-login token that the Tauri shell can inject into
    /// the webview so the user never sees the login page in desktop mode.
    pub fn local_token(&self) -> &str {
        LOCAL_AUTO_TOKEN
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
                    emit_console_log(
                        LogLevel::Warn,
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
            return self.handle_terminal_websocket(socket).await;
        }

        let request = read_http_request(&mut socket).await?;
        if request.is_empty() {
            return Ok(());
        }

        if let Some((method, raw_path)) = parse_request_line(&String::from_utf8_lossy(&request)) {
            let path = raw_path.split('?').next().unwrap_or(raw_path);
            if method.eq_ignore_ascii_case("GET") && path == LOG_REALTIME_ROUTE {
                return self.stream_realtime_logs(socket).await;
            }
            if method.eq_ignore_ascii_case("GET") && path == SYSTEM_STATUS_REALTIME_ROUTE {
                return self.stream_system_status(socket).await;
            }
        }

        let response = self.route_http_request(&request, peer_addr.ip());
        socket.write_all(&response).await?;
        socket.shutdown().await
    }

    async fn handle_terminal_websocket(&self, socket: TcpStream) -> io::Result<()> {
        let request_path = Arc::new(Mutex::new(None::<String>));
        let capture = Arc::clone(&request_path);
        let ws_stream = accept_hdr_async(socket, move |request: &Request, response: Response| {
            if let Ok(mut slot) = capture.lock() {
                *slot = request
                    .uri()
                    .path_and_query()
                    .map(|value| value.as_str().to_string())
                    .or_else(|| Some(request.uri().path().to_string()));
            }
            Ok(response)
        })
        .await
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, format!("websocket upgrade failed: {err}")))?;

        let raw_path = request_path
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .unwrap_or_default();
        let query = parse_query_string(raw_path.as_str());
        let session_id = query.get("id").cloned().unwrap_or_default();
        let token = query.get("token").cloned().unwrap_or_default();
        let Some(session) = self.terminal_state.get_session(session_id.as_str()) else {
            return serve_terminal_socket_with_error(ws_stream, "terminal session not found").await;
        };
        if token != LOCAL_AUTO_TOKEN {
            return serve_terminal_socket_with_error(ws_stream, "terminal token is invalid").await;
        }
        if let Err(err) = session
            .ensure_started(Arc::clone(&self.terminal_state))
            .await
        {
            return serve_terminal_socket_with_error(ws_stream, err.as_str()).await;
        }

        let (mut ws_write, mut ws_read) = ws_stream.split();
        if let Some(history) = session.recent_history_text() {
            ws_write
                .send(terminal_ws_text(format!("{history}\r\n")))
                .await
                .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, format!("failed to replay terminal history: {err}")))?;
        }
        ws_write
            .send(terminal_ws_text(format!(
                "\u{1b}[90m[terminal:{}] attached to local {} shell\u{1b}[0m\r\n",
                session.id, session.shell
            )))
            .await
            .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, format!("failed to write terminal banner: {err}")))?;

        let mut output_rx = session.subscribe();
        let writer = tokio::spawn(async move {
            loop {
                match output_rx.recv().await {
                    Ok(first_chunk) => {
                        let mut chunk = first_chunk;
                        loop {
                            match tokio::time::timeout(TERMINAL_WS_BATCH_INTERVAL, output_rx.recv()).await {
                                Ok(Ok(next_chunk)) => chunk.push_str(next_chunk.as_str()),
                                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                                Ok(Err(broadcast::error::RecvError::Closed)) => break,
                                Err(_) => break,
                            }
                        }
                        if ws_write.send(terminal_ws_text(chunk)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let input_session = Arc::clone(&session);
        let reader = tokio::spawn(async move {
            while let Some(message) = ws_read.next().await {
                match message {
                    Ok(Message::Text(payload)) => {
                        if let Err(err) = handle_terminal_client_message(&input_session, payload.as_ref()) {
                            let _ = input_session.output_tx.send(format!(
                                "\r\n\u{1b}[31m[terminal:{}] {}\u{1b}[0m\r\n",
                                input_session.id, err
                            ));
                        }
                    }
                    Ok(Message::Binary(payload)) => {
                        if let Ok(text) = String::from_utf8(payload.to_vec()) {
                            if let Err(err) = handle_terminal_client_message(&input_session, text.as_str()) {
                                let _ = input_session.output_tx.send(format!(
                                    "\r\n\u{1b}[31m[terminal:{}] {}\u{1b}[0m\r\n",
                                    input_session.id, err
                                ));
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                    Err(_) => break,
                }
            }
        });

        let _ = tokio::join!(writer, reader);
        Ok(())
    }

    async fn stream_realtime_logs(&self, mut socket: TcpStream) -> io::Result<()> {
        write_sse_headers(&mut socket).await?;
        let mut ticker = tokio::time::interval(REALTIME_STREAM_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut previous_entries = Vec::new();
        let mut keepalive_ticks = 0usize;

        loop {
            ticker.tick().await;
            let current_entries = recent_buffered_logs(REALTIME_LOG_STREAM_LIMIT);
            let appended = if previous_entries.is_empty() {
                current_entries.clone()
            } else {
                appended_log_entries(previous_entries.as_slice(), current_entries.as_slice())
            };

            if appended.is_empty() {
                keepalive_ticks += 1;
                if keepalive_ticks >= SSE_KEEPALIVE_TICKS {
                    write_sse_comment(&mut socket, "keep-alive").await?;
                    keepalive_ticks = 0;
                }
            } else {
                keepalive_ticks = 0;
                let payload = serde_json::json!({
                    "level": aggregate_log_level(appended.as_slice()),
                    "message": appended
                        .iter()
                        .map(|entry| entry.line.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                write_sse_event(
                    &mut socket,
                    &serde_json::to_string(&payload).unwrap_or_else(|_| "{\"level\":\"info\",\"message\":\"log serialization error\"}".to_string()),
                )
                .await?;
            }

            previous_entries = current_entries;
        }
    }

    async fn stream_system_status(&self, mut socket: TcpStream) -> io::Result<()> {
        write_sse_headers(&mut socket).await?;
        let mut ticker = tokio::time::interval(REALTIME_STREAM_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;
            let snapshot = (self.snapshot_provider)();
            let payload = serde_json::to_string(&napcat_system_status(&snapshot))
                .unwrap_or_else(|_| "{}".to_string());
            write_sse_event(&mut socket, payload.as_str()).await?;
        }
    }

    fn route_http_request(&self, request: &[u8], peer_ip: IpAddr) -> Vec<u8> {
        let request_line = String::from_utf8_lossy(request);
        let Some((method, raw_path)) = parse_request_line(&request_line) else {
            return build_response(
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"bad request",
                false,
            );
        };
        let path = raw_path.split('?').next().unwrap_or(raw_path);
        let is_head = method.eq_ignore_ascii_case("HEAD");

        // ── CORS preflight ────────────────────────────────────────────────────
        if method.eq_ignore_ascii_case("OPTIONS") {
            return options_response();
        }

        // ── Legacy LiteyukiBot routes ─────────────────────────────────────────
        if path == HEALTH_ROUTE {
            let body = serde_json::to_vec_pretty(&self.health())
                .unwrap_or_else(|_| b"{\"status\":\"serialization-error\"}".to_vec());
            return build_response("200 OK", "application/json; charset=utf-8", &body, is_head);
        }

        if path == LOGS_ROUTE {
            let body = serde_json::to_vec_pretty(&WebHostLogsPayload {
                entries: recent_buffered_logs(LOGS_ROUTE_LIMIT),
            })
            .unwrap_or_else(|_| b"{\"entries\":[]}".to_vec());
            return build_response("200 OK", "application/json; charset=utf-8", &body, is_head);
        }

        if path == I18N_ROUTE {
            let body = serde_json::to_vec_pretty(&current_i18n_snapshot())
                .unwrap_or_else(|_| b"{\"messages\":{}}".to_vec());
            return build_response("200 OK", "application/json; charset=utf-8", &body, is_head);
        }

        if path == "/files/theme.css" {
            let body = render_theme_css(&load_theme_config());
            return build_response("200 OK", "text/css; charset=utf-8", body.as_bytes(), is_head);
        }

        if let Some(asset) = built_in_public_font(path) {
            return build_response("200 OK", asset.content_type(), asset.body(), is_head);
        }

        // ── Static assets (injected at startup) ───────────────────────────────
        if let Some(asset) = self.assets.static_asset_for_path(path) {
            return build_response("200 OK", asset.content_type(), asset.body(), is_head);
        }

        // ── NapCat-compatible API routes ──────────────────────────────────────
        if path.starts_with("/api/") {
            return self.route_napcat_api(request, method, path, raw_path, is_head, peer_ip);
        }

        // ── Dev frontend redirect ─────────────────────────────────────────────
        if let Some(location) = self.dev_frontend_redirect(raw_path, path, &request_line) {
            return build_redirect_response("307 Temporary Redirect", location.as_str(), is_head);
        }

        match self.assets.asset_for_path(path) {
            Some(asset) => build_response("200 OK", asset.content_type(), asset.body(), is_head),
            None => build_response(
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"not found",
                is_head,
            ),
        }
    }

    /// Route all NapCat-compatible `/api/*` paths.
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
        // ── /files/theme.css ─────────────────────────────────────────────────
        if path == "/files/theme.css" {
            let body = render_theme_css(&load_theme_config());
            return build_response("200 OK", "text/css; charset=utf-8", body.as_bytes(), is_head);
        }

        // Strip the /api prefix for matching
        let api_path = path.strip_prefix("/api").unwrap_or(path);

        // ── Auth ─────────────────────────────────────────────────────────────
        if api_path == "/auth/check" {
            // Always report as logged-in (no real auth in this stub)
            let body = napcat_ok(&true);
            return napcat_response(body, is_head);
        }

        // Local-token endpoint: only loopback connections may fetch the token.
        // This is the server-side gate that makes the auto-login safe.
        if api_path == "/auth/local-token" {
            if peer_ip.is_loopback() {
                #[derive(Serialize)]
                struct LocalTokenResponse<'a> {
                    token: &'a str,
                }
                let body = napcat_ok(&LocalTokenResponse {
                    token: LOCAL_AUTO_TOKEN,
                });
                return napcat_response(body, is_head);
            }
            // Non-loopback: refuse with 403
            let body = napcat_err(403, "Forbidden");
            return napcat_response(body, is_head);
        }

        if api_path == "/auth/login" {
            #[derive(Serialize)]
            struct AuthResponse {
                #[serde(rename = "Credential")]
                credential: String,
            }
            let body = napcat_ok(&AuthResponse {
                credential: LOCAL_AUTO_TOKEN.to_string(),
            });
            return napcat_response(body, is_head);
        }

        if api_path == "/auth/update_token" {
            let body = napcat_ok(&true);
            return napcat_response(body, is_head);
        }

        if api_path == "/auth/passkey/generate-registration-options"
            || api_path == "/auth/passkey/verify-registration"
            || api_path == "/auth/passkey/generate-authentication-options"
            || api_path == "/auth/passkey/verify-authentication"
        {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        // ── Base / System ─────────────────────────────────────────────────────
        if api_path == "/base/GetNapCatVersion" {
            #[derive(Serialize)]
            struct PackageInfo {
                version: String,
                #[serde(rename = "buildTime")]
                build_time: String,
            }
            let body = napcat_ok(&PackageInfo {
                version: env!("CARGO_PKG_VERSION").to_string(),
                build_time: option_env!("VERGEN_BUILD_TIMESTAMP")
                    .unwrap_or("unknown")
                    .to_string(),
            });
            return napcat_response(body, is_head);
        }

        if api_path == "/base/getLatestTag" {
            let body = napcat_ok(&env!("CARGO_PKG_VERSION"));
            return napcat_response(body, is_head);
        }

        if api_path == "/base/getAllReleases" {
            #[derive(Serialize)]
            struct Pagination {
                page: u32,
                #[serde(rename = "pageSize")]
                page_size: u32,
                total: u32,
                #[serde(rename = "totalPages")]
                total_pages: u32,
            }
            #[derive(Serialize)]
            struct Releases {
                versions: Vec<serde_json::Value>,
                pagination: Pagination,
            }
            let body = napcat_ok(&Releases {
                versions: vec![],
                pagination: Pagination {
                    page: 1,
                    page_size: 20,
                    total: 0,
                    total_pages: 0,
                },
            });
            return napcat_response(body, is_head);
        }

        if api_path == "/base/getMirrors" {
            #[derive(Serialize)]
            struct Mirrors {
                mirrors: Vec<String>,
            }
            let body = napcat_ok(&Mirrors { mirrors: vec![] });
            return napcat_response(body, is_head);
        }

        if api_path == "/base/QQVersion" {
            let body = napcat_ok(&"N/A");
            return napcat_response(body, is_head);
        }

        if api_path == "/base/Theme" {
            let body = napcat_ok(&load_theme_config());
            return napcat_response(body, is_head);
        }

        if api_path == "/base/SetTheme" {
            let body = parse_json_body(request);
            let theme_value = body.get("theme").cloned().unwrap_or(body);
            let theme = serde_json::from_value::<ThemeConfigDoc>(theme_value)
                .unwrap_or_else(|_| load_theme_config());
            let result = save_theme_config(&theme).is_ok();
            let body = napcat_ok(&result);
            return napcat_response(body, is_head);
        }

        if api_path == "/base/proxy" {
            let body = napcat_ok(&"{}");
            return napcat_response(body, is_head);
        }

        if api_path == "/base/GetNapCatFileHash" {
            let body = napcat_ok(&serde_json::json!({
                "hash": "",
                "file": "",
                "algorithm": "sha256"
            }));
            return napcat_response(body, is_head);
        }

        // SSE: system status
        if api_path == "/base/GetSysStatusRealTime" {
            let snapshot = (self.snapshot_provider)();
            let status = napcat_system_status(&snapshot);
            let event_data = serde_json::to_string(&status).unwrap_or_default();
            return sse_response(&event_data, is_head);
        }

        // ── Process ───────────────────────────────────────────────────────────
        if api_path == "/Process/Restart" {
            let body = napcat_ok(&serde_json::json!({ "message": "restart requested" }));
            return napcat_response(body, is_head);
        }

        if api_path == "/UpdateNapCat/update" {
            let body = napcat_ok(
                &serde_json::json!({ "message": "Update not supported in Liteyuki" }),
            );
            return napcat_response(body, is_head);
        }

        // ── QQ Login ──────────────────────────────────────────────────────────
        if api_path == "/QQLogin/CheckLoginStatus" {
            let body = napcat_ok(&serde_json::json!({
                "isLogin": false,
                "isOffline": false,
                "qrcodeurl": ""
            }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/RefreshQRcode" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetQQLoginQrcode" {
            let body = napcat_ok(&serde_json::json!({ "qrcode": "" }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetQuickLoginList" {
            let body = napcat_ok(&Vec::<String>::new());
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetQuickLoginListNew" {
            let snapshot = (self.snapshot_provider)();
            let body = napcat_ok(&vec![serde_json::json!({
                "uin": "local-webui",
                "uid": "local-webui",
                "nickName": snapshot.app_name,
                "faceUrl": "",
                "facePath": "",
                "loginType": 1,
                "isQuickLogin": true,
                "isAutoLogin": false
            })]);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/SetQuickLogin" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetQQLoginInfo" {
            let snapshot = (self.snapshot_provider)();
            let body = napcat_ok(&serde_json::json!({
                "uid": "local-webui",
                "uin": "local-webui",
                "nick": snapshot.app_name,
                "avatarUrl": serde_json::Value::Null,
                "online": snapshot.status == "running"
            }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetQuickLoginQQ" {
            let body = napcat_ok(&"local-webui");
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/SetQuickLoginQQ" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/PasswordLogin" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/CaptchaLogin" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/NewDeviceLogin" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetNewDeviceQRCode" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/PollNewDeviceQR" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/ResetDeviceID" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/RestartNapCat" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetDeviceGUID" {
            let body = napcat_ok(&serde_json::json!({ "guid": "" }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/SetDeviceGUID" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetGUIDBackups" {
            let body = napcat_ok(&Vec::<String>::new());
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/RestoreGUIDBackup" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/CreateGUIDBackup" {
            let body = napcat_ok(&serde_json::json!({ "path": "" }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetPlatformInfo" {
            let body = napcat_ok(&serde_json::json!({ "platform": std::env::consts::OS }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetLinuxMAC" {
            let body = napcat_ok(&serde_json::json!({ "mac": "" }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/SetLinuxMAC" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetLinuxMachineId" {
            let body = napcat_ok(&serde_json::json!({ "machineId": "" }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/ComputeLinuxGUID" {
            let body = napcat_ok(&serde_json::json!({ "guid": "", "machineId": "", "mac": "" }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetLinuxMachineInfoBackups" {
            let body = napcat_ok(&Vec::<String>::new());
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/CreateLinuxMachineInfoBackup" {
            let body = napcat_ok(&serde_json::json!({ "path": "" }));
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/RestoreLinuxMachineInfoBackup" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/ResetLinuxDeviceID" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/QQLogin/GetAllUsers" {
            let snapshot = (self.snapshot_provider)();
            let body = napcat_ok(&vec![serde_json::json!({
                "uin": "local-webui",
                "uid": "local-webui",
                "nick": snapshot.app_name,
                "avatarUrl": serde_json::Value::Null,
                "online": snapshot.status == "running"
            })]);
            return napcat_response(body, is_head);
        }

        // ── OB11 Config ───────────────────────────────────────────────────────
        if api_path == "/OB11Config/GetConfig" {
            let config = load_onebot_config();
            let body = napcat_ok(&config);
            return napcat_response(body, is_head);
        }

        if api_path == "/OB11Config/SetConfig" {
            let body = parse_json_body(request);
            let config_value = body
                .get("config")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .or_else(|| body.get("config").cloned())
                .unwrap_or(Value::Null);
            if let Ok(config) = serde_json::from_value::<OneBotConfig>(config_value) {
                let _ = save_onebot_config(&config);
            }
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        // ── NapCat Config ─────────────────────────────────────────────────────
        if api_path == "/NapCatConfig/GetConfig" || api_path == "/NapCatConfig/GetUinConfig" {
            let config = load_napcat_config(api_path == "/NapCatConfig/GetUinConfig");
            let body = napcat_ok(&config);
            return napcat_response(body, is_head);
        }

        if api_path == "/NapCatConfig/SetConfig" || api_path == "/NapCatConfig/SetUinConfig" {
            let body = parse_json_body(request);
            if let Ok(config) = serde_json::from_value::<NapCatConfig>(body) {
                let _ = save_napcat_config(api_path == "/NapCatConfig/SetUinConfig", &config);
            }
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        // ── WebUI Config ──────────────────────────────────────────────────────
        if api_path == "/WebUIConfig/GetConfig" {
            let config = load_webui_server_config(self.bind_addr.port());
            let body = napcat_ok(&config);
            return napcat_response(body, is_head);
        }

        if api_path == "/WebUIConfig/UpdateConfig" {
            let mut config = load_webui_server_config(self.bind_addr.port());
            let body = parse_json_body(request);
            if let Some(host) = body.get("host").and_then(Value::as_str) {
                config.host = host.trim().to_string();
            }
            if let Some(port) = body.get("port").and_then(Value::as_u64) {
                config.port = port as u16;
            }
            if let Some(login_rate) = body.get("loginRate").and_then(Value::as_u64) {
                config.login_rate = login_rate as u32;
            }
            if let Some(disable) = body.get("disableWebUI").and_then(Value::as_bool) {
                config.disable_webui = disable;
            }
            if let Some(mode) = body.get("accessControlMode").and_then(Value::as_str) {
                config.access_control_mode = mode.to_string();
            }
            if let Some(whitelist) = body.get("ipWhitelist").and_then(Value::as_array) {
                config.ip_whitelist = whitelist
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect();
            }
            if let Some(blacklist) = body.get("ipBlacklist").and_then(Value::as_array) {
                config.ip_blacklist = blacklist
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect();
            }
            if let Some(enabled) = body.get("enableXForwardedFor").and_then(Value::as_bool) {
                config.enable_x_forwarded_for = enabled;
            }
            let _ = save_webui_server_config(&config);
            let body = napcat_ok(&true);
            return napcat_response(body, is_head);
        }

        if api_path == "/WebUIConfig/GetDisableWebUI" {
            let body = napcat_ok(&load_webui_server_config(self.bind_addr.port()).disable_webui);
            return napcat_response(body, is_head);
        }

        if api_path == "/WebUIConfig/UpdateDisableWebUI" {
            let mut config = load_webui_server_config(self.bind_addr.port());
            if let Some(disable) = parse_json_body(request).get("disable").and_then(Value::as_bool) {
                config.disable_webui = disable;
                let _ = save_webui_server_config(&config);
            }
            let body = napcat_ok(&true);
            return napcat_response(body, is_head);
        }

        if api_path == "/WebUIConfig/GetClientIP" {
            let request_str = String::from_utf8_lossy(request);
            let config = load_webui_server_config(self.bind_addr.port());
            let ip = if config.enable_x_forwarded_for {
                extract_header(&request_str, "X-Forwarded-For")
                    .unwrap_or("127.0.0.1")
                    .to_string()
            } else {
                peer_ip.to_string()
            };
            let body = napcat_ok(&serde_json::json!({ "ip": ip }));
            return napcat_response(body, is_head);
        }

        if api_path == "/WebUIConfig/GetSSLStatus" {
            let cert_path = state_path(SSL_CERT_FILE);
            let key_path = state_path(SSL_KEY_FILE);
            let cert_content = fs::read_to_string(&cert_path).unwrap_or_default();
            let key_content = fs::read_to_string(&key_path).unwrap_or_default();
            let body = napcat_ok(&serde_json::json!({
                "enabled": cert_path.is_file() && key_path.is_file(),
                "certExists": cert_path.is_file(),
                "keyExists": key_path.is_file(),
                "certContent": cert_content,
                "keyContent": key_content
            }));
            return napcat_response(body, is_head);
        }

        if api_path == "/WebUIConfig/UploadSSLCert" {
            let body = parse_json_body(request);
            let cert = body.get("cert").and_then(Value::as_str).unwrap_or_default();
            let key = body.get("key").and_then(Value::as_str).unwrap_or_default();
            let result = if cert.trim().is_empty() || key.trim().is_empty() {
                Err("certificate or private key is empty".to_string())
            } else {
                fs::create_dir_all(state_path(WEBUI_STATE_DIR))
                    .map_err(|err| format!("failed to create webui state dir: {err}"))
                    .and_then(|_| {
                        fs::write(state_path(SSL_CERT_FILE), cert)
                            .map_err(|err| format!("failed to write cert: {err}"))?;
                        fs::write(state_path(SSL_KEY_FILE), key)
                            .map_err(|err| format!("failed to write key: {err}"))
                    })
            };
            let body = napcat_ok(&serde_json::json!({
                "message": result.map(|_| "SSL certificate saved".to_string()).unwrap_or_else(|err| err)
            }));
            return napcat_response(body, is_head);
        }

        if api_path == "/WebUIConfig/DeleteSSLCert" {
            let _ = fs::remove_file(state_path(SSL_CERT_FILE));
            let _ = fs::remove_file(state_path(SSL_KEY_FILE));
            let body = napcat_ok(&serde_json::json!({ "message": "SSL certificate deleted" }));
            return napcat_response(body, is_head);
        }

        // ── Log ───────────────────────────────────────────────────────────────
        if api_path == "/Log/GetLogList" {
            let body = napcat_ok(&vec!["runtime.log".to_string()]);
            return napcat_response(body, is_head);
        }

        if api_path.starts_with("/Log/GetLog") && !api_path.contains("RealTime") {
            let query = parse_query_string(raw_path);
            let log_id = query.get("id").cloned().unwrap_or_default();
            let body = napcat_ok(&if log_id.is_empty() || log_id == "runtime.log" {
                format_log_history(LOGS_ROUTE_LIMIT.max(400))
            } else {
                String::new()
            });
            return napcat_response(body, is_head);
        }

        // SSE: real-time logs
        if api_path == "/Log/GetLogRealTime" {
            let entries = recent_buffered_logs(50);
            let event_data = if let Some(last) = entries.last() {
                serde_json::json!({
                    "level": last.level,
                    "message": last.message
                })
                .to_string()
            } else {
                serde_json::json!({ "level": "info", "message": "Liteyuki running" })
                    .to_string()
            };
            return sse_response(&event_data, is_head);
        }

        // Terminal (WebSocket is handled separately; these are the REST endpoints)
        if api_path == "/Log/terminal/create" {
            let body = parse_json_body(request);
            let cols = body
                .get("cols")
                .and_then(Value::as_u64)
                .map(|value| value as u16)
                .unwrap_or(TERMINAL_DEFAULT_COLS);
            let rows = body
                .get("rows")
                .and_then(Value::as_u64)
                .map(|value| value as u16)
                .unwrap_or(TERMINAL_DEFAULT_ROWS);
            let id = self.terminal_state.create_session(cols, rows);
            let body = napcat_ok(&serde_json::json!({ "id": id }));
            return napcat_response(body, is_head);
        }

        if api_path == "/Log/terminal/list" {
            let body = napcat_ok(
                &self
                    .terminal_state
                    .list_sessions()
                    .into_iter()
                    .map(|id| serde_json::json!({ "id": id }))
                    .collect::<Vec<_>>(),
            );
            return napcat_response(body, is_head);
        }

        if api_path.starts_with("/Log/terminal/") && api_path.ends_with("/close") {
            let terminal_id = api_path
                .strip_prefix("/Log/terminal/")
                .and_then(|value| value.strip_suffix("/close"))
                .unwrap_or_default();
            let closed = self.terminal_state.close_session(terminal_id);
            let body = napcat_ok(&closed);
            return napcat_response(body, is_head);
        }

        // ── File ──────────────────────────────────────────────────────────────
        if api_path.starts_with("/File/") {
            return self.route_file_api(method, api_path, raw_path, request, is_head);
        }

        // ── Plugin ────────────────────────────────────────────────────────────
        if api_path == "/Plugin/List" {
            let plugins = discover_plugins();
            let body = napcat_ok(&serde_json::json!({
                "plugins": plugins,
                "pluginManagerNotFound": false,
                "extensionPages": []
            }));
            return napcat_response(body, is_head);
        }

        if api_path == "/Plugin/RegisterManager" {
            let body = napcat_ok(&serde_json::json!({
                "message": format!("plugin manager ready ({} discovered)", discover_plugins().len())
            }));
            return napcat_response(body, is_head);
        }

        if api_path == "/Plugin/SetStatus" {
            let body = parse_json_body(request);
            if let (Some(id), Some(enable)) = (
                body.get("id").and_then(Value::as_str),
                body.get("enable").and_then(Value::as_bool),
            ) {
                let _ = update_disabled_plugins(id, enable);
            }
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/Plugin/Uninstall" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/Plugin/Import" {
            let body = napcat_ok(&serde_json::json!({
                "message": "Plugin import not supported",
                "pluginId": "",
                "installPath": ""
            }));
            return napcat_response(body, is_head);
        }

        if api_path == "/Plugin/Store/List" {
            let body = napcat_ok(&serde_json::json!({ "plugins": [] }));
            return napcat_response(body, is_head);
        }

        if api_path.starts_with("/Plugin/Store/Detail/") {
            let body = napcat_err(-1, "Plugin not found");
            return napcat_response(body, is_head);
        }

        if api_path == "/Plugin/Store/Install" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/Plugin/Config" {
            if method.eq_ignore_ascii_case("GET") {
                let body = napcat_ok(&serde_json::json!({
                    "schema": [],
                    "config": {},
                    "supportReactive": false
                }));
                return napcat_response(body, is_head);
            }
            // POST: set config
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/Plugin/Config/Change" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        // SSE: plugin config
        if api_path == "/Plugin/Config/SSE" {
            let event_data = serde_json::json!({ "type": "complete" }).to_string();
            return sse_response(&event_data, is_head);
        }

        // ── Mirror ────────────────────────────────────────────────────────────
        if api_path == "/Mirror/List" {
            let body = napcat_ok(&serde_json::json!({
                "fileMirrors": [],
                "rawMirrors": [],
                "customMirror": null,
                "timeout": 5000
            }));
            return napcat_response(body, is_head);
        }

        if api_path == "/Mirror/SetCustom" {
            let body = napcat_ok(&serde_json::Value::Null);
            return napcat_response(body, is_head);
        }

        if api_path == "/Mirror/Test" {
            let body = napcat_ok(&serde_json::json!({
                "mirror": "",
                "latency": 0,
                "success": false,
                "error": "Mirror testing not supported"
            }));
            return napcat_response(body, is_head);
        }

        // SSE: mirror test
        if api_path == "/Mirror/Test/SSE" {
            let event_data = serde_json::json!({
                "type": "complete",
                "results": [],
                "failed": [],
                "fastest": null,
                "message": "Mirror testing not supported"
            })
            .to_string();
            return sse_response(&event_data, is_head);
        }

        // ── Debug WebSocket (handled by proxy in dev; stub for prod) ──────────
        if api_path == "/Debug/ws" || api_path == "/ws/terminal" {
            return build_response(
                "426 Upgrade Required",
                "text/plain; charset=utf-8",
                b"WebSocket upgrade required",
                is_head,
            );
        }

        // ── Fallthrough: unknown /api/* ────────────────────────────────────────
        let body = napcat_err(-1, "not found");
        napcat_response(body, is_head)
    }

    /// Handle `/api/File/*` routes.
    fn route_file_api(
        &self,
        method: &str,
        api_path: &str,
        raw_path: &str,
        _request: &[u8],
        is_head: bool,
    ) -> Vec<u8> {
        let query = parse_query_string(raw_path);
        // GET endpoints
        if method.eq_ignore_ascii_case("GET") {
            if api_path == "/File/list" {
                let target = query.get("path").map(String::as_str).unwrap_or("/");
                let only_directory = query
                    .get("onlyDirectory")
                    .map(|value| value.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                let body = match resolve_workspace_path(target)
                    .and_then(|path| {
                        ensure_path_within_workspace(path.as_path())?;
                        let mut items = fs::read_dir(path)
                            .map_err(|err| format!("failed to read workspace directory: {err}"))?
                            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                            .filter_map(|entry| build_workspace_file_info(entry.as_path()).ok())
                            .filter(|entry| !only_directory || entry.is_directory)
                            .collect::<Vec<_>>();
                        items.sort_by(|left, right| {
                            left.is_directory
                                .cmp(&right.is_directory)
                                .reverse()
                                .then_with(|| left.name.cmp(&right.name))
                        });
                        Ok(items)
                    }) {
                    Ok(items) => napcat_ok(&items),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/read" {
                let target = query.get("path").map(String::as_str).unwrap_or("/");
                let body = match resolve_workspace_path(target)
                    .and_then(|path| {
                        ensure_path_within_workspace(path.as_path())?;
                        fs::read_to_string(path)
                            .map_err(|err| format!("failed to read file: {err}"))
                    }) {
                    Ok(content) => napcat_ok(&content),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/font/exists/webui" {
                let body = napcat_ok(&state_path(CUSTOM_FONT_FILE).is_file());
                return napcat_response(body, is_head);
            }
            if api_path.starts_with("/File/download") {
                let target = query.get("path").map(String::as_str).unwrap_or("/");
                if let Ok(path) = resolve_workspace_path(target)
                    .and_then(|path| {
                        ensure_path_within_workspace(path.as_path())?;
                        Ok(path)
                    })
                    && let Ok(bytes) = fs::read(path)
                {
                    return build_response(
                        "200 OK",
                        "application/octet-stream",
                        bytes.as_slice(),
                        is_head,
                    );
                }
                return build_response(
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    b"file not found",
                    is_head,
                );
            }
        }

        // POST endpoints
        if method.eq_ignore_ascii_case("POST") {
            let body = parse_json_body(_request);
            if api_path.starts_with("/File/download") {
                let target = query.get("path").map(String::as_str).unwrap_or("/");
                if let Ok(path) = resolve_workspace_path(target)
                    .and_then(|path| {
                        ensure_path_within_workspace(path.as_path())?;
                        Ok(path)
                    })
                    && let Ok(bytes) = fs::read(path)
                {
                    return build_response(
                        "200 OK",
                        "application/octet-stream",
                        bytes.as_slice(),
                        is_head,
                    );
                }
                return build_response(
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    b"file not found",
                    is_head,
                );
            }
            if api_path == "/File/mkdir" {
                let result = body
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "path is required".to_string())
                    .and_then(resolve_workspace_path)
                    .and_then(|path| {
                        if path.exists() {
                            return Ok(false);
                        }
                        ensure_path_within_workspace(path.as_path())?;
                        fs::create_dir_all(path)
                            .map_err(|err| format!("failed to create directory: {err}"))?;
                        Ok(true)
                    });
                let body = match result {
                    Ok(created) => napcat_ok(&created),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/delete" {
                let result = body
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "path is required".to_string())
                    .and_then(resolve_workspace_path)
                    .and_then(|path| {
                        ensure_path_within_workspace(path.as_path())?;
                        let metadata = fs::metadata(&path)
                            .map_err(|err| format!("failed to stat path: {err}"))?;
                        if metadata.is_dir() {
                            fs::remove_dir_all(path)
                                .map_err(|err| format!("failed to remove directory: {err}"))
                        } else {
                            fs::remove_file(path)
                                .map_err(|err| format!("failed to remove file: {err}"))
                        }
                    });
                let body = match result {
                    Ok(()) => napcat_ok(&true),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/write" {
                let result = body
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "path is required".to_string())
                    .and_then(|raw_path| {
                        let content = body
                            .get("content")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let path = resolve_workspace_path(raw_path)?;
                        if let Some(parent) = path.parent() {
                            fs::create_dir_all(parent).map_err(|err| {
                                format!("failed to create parent directory: {err}")
                            })?;
                        }
                        ensure_path_within_workspace(parent_or_self(path.as_path()).as_path())?;
                        fs::write(path, content)
                            .map_err(|err| format!("failed to write file: {err}"))
                    });
                let body = match result {
                    Ok(()) => napcat_ok(&true),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/create" {
                let result = body
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "path is required".to_string())
                    .and_then(resolve_workspace_path)
                    .and_then(|path| {
                        if path.exists() {
                            return Ok(false);
                        }
                        if let Some(parent) = path.parent() {
                            fs::create_dir_all(parent).map_err(|err| {
                                format!("failed to create parent directory: {err}")
                            })?;
                        }
                        ensure_path_within_workspace(parent_or_self(path.as_path()).as_path())?;
                        fs::write(path, "")
                            .map_err(|err| format!("failed to create file: {err}"))?;
                        Ok(true)
                    });
                let body = match result {
                    Ok(created) => napcat_ok(&created),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/batchDelete" {
                let result = body
                    .get("paths")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "paths is required".to_string())
                    .and_then(|paths| {
                        for raw in paths.iter().filter_map(Value::as_str) {
                            let path = resolve_workspace_path(raw)?;
                            ensure_path_within_workspace(path.as_path())?;
                            if let Ok(metadata) = fs::metadata(&path) {
                                if metadata.is_dir() {
                                    fs::remove_dir_all(&path).map_err(|err| {
                                        format!("failed to remove directory {}: {err}", path.display())
                                    })?;
                                } else {
                                    fs::remove_file(&path).map_err(|err| {
                                        format!("failed to remove file {}: {err}", path.display())
                                    })?;
                                }
                            }
                        }
                        Ok(())
                    });
                let body = match result {
                    Ok(()) => napcat_ok(&true),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/rename" || api_path == "/File/move" {
                let from_key = if api_path == "/File/rename" {
                    "oldPath"
                } else {
                    "sourcePath"
                };
                let to_key = if api_path == "/File/rename" {
                    "newPath"
                } else {
                    "targetPath"
                };
                let result = body
                    .get(from_key)
                    .and_then(Value::as_str)
                    .zip(body.get(to_key).and_then(Value::as_str))
                    .ok_or_else(|| "source and target path are required".to_string())
                    .and_then(|(from, to)| {
                        let from_path = resolve_workspace_path(from)?;
                        let to_path = resolve_workspace_path(to)?;
                        ensure_path_within_workspace(from_path.as_path())?;
                        ensure_path_within_workspace(parent_or_self(to_path.as_path()).as_path())?;
                        if let Some(parent) = to_path.parent() {
                            fs::create_dir_all(parent).map_err(|err| {
                                format!("failed to create parent directory: {err}")
                            })?;
                        }
                        fs::rename(from_path, to_path)
                            .map_err(|err| format!("failed to move path: {err}"))
                    });
                let body = match result {
                    Ok(()) => napcat_ok(&true),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/batchMove" {
                let result = body
                    .get("items")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "items is required".to_string())
                    .and_then(|items| {
                        for item in items {
                            let from = item.get("sourcePath").and_then(Value::as_str).ok_or_else(|| {
                                "sourcePath is required".to_string()
                            })?;
                            let to = item.get("targetPath").and_then(Value::as_str).ok_or_else(|| {
                                "targetPath is required".to_string()
                            })?;
                            let from_path = resolve_workspace_path(from)?;
                            let to_path = resolve_workspace_path(to)?;
                            ensure_path_within_workspace(from_path.as_path())?;
                            ensure_path_within_workspace(parent_or_self(to_path.as_path()).as_path())?;
                            if let Some(parent) = to_path.parent() {
                                fs::create_dir_all(parent).map_err(|err| {
                                    format!("failed to create parent directory: {err}")
                                })?;
                            }
                            fs::rename(from_path, to_path)
                                .map_err(|err| format!("failed to move path: {err}"))?;
                        }
                        Ok(())
                    });
                let body = match result {
                    Ok(()) => napcat_ok(&true),
                    Err(err) => napcat_err(-1, err.as_str()),
                };
                return napcat_response(body, is_head);
            }
            if api_path == "/File/font/delete/webui" {
                let _ = fs::remove_file(state_path(CUSTOM_FONT_FILE));
                let body = napcat_ok(&true);
                return napcat_response(body, is_head);
            }
            if api_path.starts_with("/File/upload") || api_path == "/File/font/upload/webui" {
                let body = napcat_err(-1, "multipart upload is not implemented yet");
                return napcat_response(body, is_head);
            }
            if api_path == "/File/batchDownload" {
                return build_response(
                    "200 OK",
                    "application/octet-stream",
                    WORKSPACE_FILE_DOWNLOAD_NAME.as_bytes(),
                    is_head,
                );
            }
        }

        let body = napcat_err(-1, "not found");
        napcat_response(body, is_head)
    }

    fn dev_frontend_redirect(&self, raw_path: &str, path: &str, request: &str) -> Option<String> {
        if path.starts_with("/api") {
            return None;
        }

        let dev_frontend = self.dev_frontend?;
        if !dev_frontend_is_available(dev_frontend) {
            return None;
        }

        let authority = request
            .lines()
            .find_map(|line| parse_named_header(line, "Host"))
            .map(|host| rewrite_host_port(host, dev_frontend.public_port))
            .unwrap_or_else(|| format!("{}:{}", self.browser_ip, dev_frontend.public_port));

        Some(format!("http://{authority}{raw_path}"))
    }
}

async fn serve_terminal_socket_with_error<S>(
    mut ws_stream: tokio_tungstenite::WebSocketStream<S>,
    message: &str,
) -> io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let _ = ws_stream
        .send(terminal_ws_text(format!(
            "\u{1b}[31m[terminal] {}\u{1b}[0m\r\n",
            message
        )))
        .await;
    let _ = ws_stream.close(None).await;
    Ok(())
}

fn appended_log_entries(
    previous: &[BufferedLogEntry],
    current: &[BufferedLogEntry],
) -> Vec<BufferedLogEntry> {
    let max_overlap = previous.len().min(current.len());
    for overlap in (0..=max_overlap).rev() {
        if previous[previous.len().saturating_sub(overlap)..] == current[..overlap] {
            return current[overlap..].to_vec();
        }
    }
    current.to_vec()
}

fn aggregate_log_level(entries: &[BufferedLogEntry]) -> &'static str {
    let mut highest = LogLevel::Info;
    for entry in entries {
        if let Some(level) = LogLevel::parse(entry.level.as_str())
            && level > highest
        {
            highest = level;
        }
    }
    match highest {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

fn normalize_asset_path(path: impl Into<String>) -> String {
    let path = path.into();
    if path.is_empty() || path == "/" {
        "/".to_string()
    } else if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    }
}

fn dev_frontend_is_available(dev_frontend: WebHostDevServer) -> bool {
    StdTcpStream::connect_timeout(&dev_frontend.probe_addr, DEV_FRONTEND_PROBE_TIMEOUT).is_ok()
}

fn rewrite_host_port(host: &str, port: u16) -> String {
    if let Some(stripped) = host.strip_prefix('[')
        && let Some((address, _)) = stripped.split_once("]:")
    {
        return format!("[{address}]:{port}");
    }

    if host.starts_with('[') && host.ends_with(']') {
        return format!("{host}:{port}");
    }

    if let Some((hostname, _)) = host.rsplit_once(':')
        && !hostname.contains(':')
    {
        return format!("{hostname}:{port}");
    }

    format!("{host}:{port}")
}

fn load_directory_index_asset(directory: &WebHostAssetDirectory) -> Option<WebHostAsset> {
    read_asset_file(directory.root.join("index.html"))
}

fn load_directory_asset_for_request(
    directory: &WebHostAssetDirectory,
    path: &str,
) -> Option<WebHostAsset> {
    let relative_path = sanitize_request_path(path)?;
    read_asset_file(directory.root.join(relative_path))
}

fn sanitize_request_path(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let mut output = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(segment) => output.push(segment),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return None,
        }
    }

    if output.as_os_str().is_empty() {
        None
    } else {
        Some(output)
    }
}

fn read_asset_file(path: PathBuf) -> Option<WebHostAsset> {
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }

    let body = fs::read(&path).ok()?;
    Some(WebHostAsset::binary(
        guess_content_type(path.as_path()),
        body,
    ))
}

fn should_fallback_to_index(path: &str) -> bool {
    let normalized = path.trim();
    !normalized
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .contains('.')
}

fn guess_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml; charset=utf-8",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("map") => "application/json; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_HTML: &str = "<!doctype html><title>Shared Host</title>";
    const TEST_SVG: &str = "<svg viewBox=\"0 0 1 1\"></svg>";

    fn test_assets() -> WebHostAssets {
        WebHostAssets::new(WebHostAsset::text("text/html; charset=utf-8", TEST_HTML))
            .with_asset(
                "/assets/bot.svg",
                WebHostAsset::text("image/svg+xml; charset=utf-8", TEST_SVG),
            )
            .with_asset(
                "/favicon.ico",
                WebHostAsset::binary("image/x-icon", [1_u8, 2_u8, 3_u8]),
            )
    }

    fn test_snapshot() -> AppHostSnapshot {
        AppHostSnapshot {
            app_name: "Liteyuki".to_string(),
            status: "running".to_string(),
            runtime_target: "tauri2".to_string(),
            adapter_count: 3,
            resource_usage: crate::app_host::AppHostResourceUsage {
                cpu: crate::app_host::AppHostCpuUsage {
                    system_percent: 63.2,
                    process_percent: 18.6,
                },
                memory: crate::app_host::AppHostMemoryUsage {
                    total_bytes: 16 * 1024 * 1024 * 1024,
                    used_bytes: 7 * 1024 * 1024 * 1024,
                    process_bytes: 512 * 1024 * 1024,
                    system_percent: 43.75,
                    process_percent: 3.125,
                },
            },
            ..AppHostSnapshot::default()
        }
    }

    fn test_server_with_snapshot(snapshot: AppHostSnapshot) -> WebHostService {
        WebHostService {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
            browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            dev_frontend: None,
            snapshot_provider: Arc::new(move || snapshot.clone()),
            assets: Arc::new(test_assets()),
            terminal_state: Arc::new(WebTerminalState::default()),
        }
    }

    fn test_server() -> WebHostService {
        test_server_with_snapshot(test_snapshot())
    }

    fn split_response(response: Vec<u8>) -> (String, Vec<u8>) {
        let Some(split_at) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
            panic!("response did not include header separator");
        };
        let body_start = split_at + 4;
        let headers = String::from_utf8(response[..body_start].to_vec())
            .expect("headers should be valid utf8");
        let body = response[body_start..].to_vec();
        (headers, body)
    }

    fn assert_json_number_close(value: &serde_json::Value, expected: f64) {
        let actual = value
            .as_f64()
            .expect("json value should be a floating-point number");
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn root_route_returns_injected_html() {
        let response = test_server().route_http_request(
            b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body = String::from_utf8(body).expect("body should be utf8");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert_eq!(body, TEST_HTML);
    }

    #[test]
    fn health_route_returns_runtime_metadata() {
        let response = test_server().route_http_request(
            b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("health body should be valid json");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(body["runtime"]["runtime_target"], "tauri2");
        assert_eq!(body["runtime"]["status"], "running");
        assert_eq!(body["runtime"]["adapter_count"], 3);
        assert_json_number_close(
            &body["runtime"]["resource_usage"]["cpu"]["system_percent"],
            63.2,
        );
        assert_eq!(body["bind"], "0.0.0.0:14500");
        assert_eq!(body["desktop_url"], "http://127.0.0.1:14500/");
    }

    #[test]
    fn system_status_route_returns_frontend_compatible_shape() {
        let response = test_server().route_http_request(
            b"GET /api/base/GetSysStatusRealTime HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body = String::from_utf8(body).expect("sse body should be utf8");
        let payload = body
            .strip_prefix("data: ")
            .and_then(|value| value.strip_suffix("\n\n"))
            .expect("sse body should contain one data event");
        let payload: serde_json::Value =
            serde_json::from_str(payload).expect("system status event should be valid json");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("Content-Type: text/event-stream; charset=utf-8\r\n"));
        assert_json_number_close(&payload["cpu"]["usage"]["system"], 63.2);
        assert_json_number_close(&payload["cpu"]["usage"]["qq"], 18.6);
        assert_eq!(payload["memory"]["total"], 16_384);
        assert_eq!(payload["memory"]["usage"]["system"], 7_168);
        assert_eq!(payload["memory"]["usage"]["qq"], 512);
        assert_eq!(payload["arch"], arch_label());
        assert!(
            payload["cpu"]["core"]
                .as_u64()
                .is_some_and(|value| value >= 1),
            "cpu core count should be populated, got {payload:?}"
        );
        assert!(
            payload["cpu"]["model"]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty()),
            "cpu model should not be empty, got {payload:?}"
        );
    }

    #[test]
    fn qq_login_info_route_uses_runtime_identity() {
        let response = test_server().route_http_request(
            b"POST /api/QQLogin/GetQQLoginInfo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n{}",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("qq login info body should be valid json");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(body["code"], 0);
        assert_eq!(body["data"]["nick"], "Liteyuki");
        assert_eq!(body["data"]["uin"], "local-webui");
        assert_eq!(body["data"]["uid"], "local-webui");
        assert_eq!(body["data"]["online"], true);
        assert!(body["data"]["avatarUrl"].is_null());
    }

    #[test]
    fn theme_css_route_returns_generated_stylesheet() {
        let response = test_server().route_http_request(
            b"GET /files/theme.css HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body = String::from_utf8(body).expect("theme css body should be utf8");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("Content-Type: text/css; charset=utf-8\r\n"));
        assert!(body.contains("--font-family-base"));
    }

    #[test]
    fn public_font_route_serves_builtin_webui_fonts() {
        let response = test_server().route_http_request(
            b"GET /webui/fonts/AaCute.woff HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("Content-Type: font/woff\r\n"));
        assert!(!body.is_empty());
    }

    #[test]
    fn logs_route_returns_recent_buffered_entries() {
        let unique = format!(
            "web-host-log-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        );
        crate::emit_console_log(crate::LogLevel::Info, "web.host.test", unique.as_str());

        let response = test_server().route_http_request(
            b"GET /api/logs HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("logs body should be valid json");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(
            body["entries"]
                .as_array()
                .is_some_and(|entries| entries.iter().any(|entry| entry["message"] == unique)),
            "expected buffered log entry in response, got {body:?}"
        );
    }

    #[test]
    fn terminal_routes_create_list_and_close_sessions() {
        let server = test_server();

        let create_response = server.route_http_request(
            br#"POST /api/Log/terminal/create HTTP/1.1
Host: localhost
Content-Type: application/json
Content-Length: 21

{"cols":100,"rows":30}"#,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (_, create_body) = split_response(create_response);
        let create_body: serde_json::Value =
            serde_json::from_slice(&create_body).expect("create body should be valid json");
        let terminal_id = create_body["data"]["id"]
            .as_str()
            .expect("terminal id should be returned")
            .to_string();
        assert!(terminal_id.starts_with("term-"));

        let list_response = server.route_http_request(
            b"GET /api/Log/terminal/list HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (_, list_body) = split_response(list_response);
        let list_body: serde_json::Value =
            serde_json::from_slice(&list_body).expect("list body should be valid json");
        assert!(
            list_body["data"]
                .as_array()
                .is_some_and(|items| items.iter().any(|entry| entry["id"] == terminal_id)),
            "created terminal should appear in list, got {list_body:?}"
        );

        let close_response = server.route_http_request(
            format!(
                "POST /api/Log/terminal/{terminal_id}/close HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n"
            )
            .as_bytes(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (_, close_body) = split_response(close_response);
        let close_body: serde_json::Value =
            serde_json::from_slice(&close_body).expect("close body should be valid json");
        assert_eq!(close_body["data"], true);

        let list_response = server.route_http_request(
            b"GET /api/Log/terminal/list HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (_, list_body) = split_response(list_response);
        let list_body: serde_json::Value =
            serde_json::from_slice(&list_body).expect("list body should be valid json");
        assert!(
            list_body["data"]
                .as_array()
                .is_some_and(|items| items.iter().all(|entry| entry["id"] != terminal_id)),
            "closed terminal should be removed from list, got {list_body:?}"
        );
    }

    #[test]
    fn appended_log_entries_handles_ring_buffer_rotation() {
        let entry = |line: &str| BufferedLogEntry {
            timestamp: "2026-04-22T00:00:00Z".to_string(),
            level: "INFO".to_string(),
            module: "web.host.test".to_string(),
            message: line.to_string(),
            line: line.to_string(),
        };

        let previous = vec![entry("a"), entry("b"), entry("c")];
        let current = vec![entry("b"), entry("c"), entry("d"), entry("e")];
        let appended = appended_log_entries(previous.as_slice(), current.as_slice());

        assert_eq!(
            appended.into_iter().map(|item| item.message).collect::<Vec<_>>(),
            vec!["d".to_string(), "e".to_string()]
        );
    }

    #[test]
    fn terminal_history_replays_only_last_three_lines() {
        let session = TerminalSession::new(
            "term-test".to_string(),
            TERMINAL_DEFAULT_COLS,
            TERMINAL_DEFAULT_ROWS,
            preferred_terminal_shell(),
        );

        session.remember_output("line-1\r\nline-2\r\n");
        session.remember_output("line-3\r\nline-4");

        assert_eq!(
            session.recent_history_text().as_deref(),
            Some("line-2\r\nline-3\r\nline-4")
        );
    }

    #[test]
    fn aggregate_log_level_prefers_highest_severity_in_batch() {
        let entries = vec![
            BufferedLogEntry {
                timestamp: "2026-04-22T00:00:00Z".to_string(),
                level: "INFO".to_string(),
                module: "web.host.test".to_string(),
                message: "info".to_string(),
                line: "info".to_string(),
            },
            BufferedLogEntry {
                timestamp: "2026-04-22T00:00:01Z".to_string(),
                level: "WARN".to_string(),
                module: "web.host.test".to_string(),
                message: "warn".to_string(),
                line: "warn".to_string(),
            },
            BufferedLogEntry {
                timestamp: "2026-04-22T00:00:02Z".to_string(),
                level: "ERROR".to_string(),
                module: "web.host.test".to_string(),
                message: "error".to_string(),
                line: "error".to_string(),
            },
        ];

        assert_eq!(aggregate_log_level(entries.as_slice()), "error");
    }

    #[test]
    fn ob11_config_route_returns_napcat_compatible_shape() {
        let response = test_server().route_http_request(
            b"GET /api/OB11Config/GetConfig HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("ob11 config body should be valid json");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(body["code"], 0);
        assert_eq!(
            body["data"]["network"]["httpServers"],
            serde_json::json!([])
        );
        assert_eq!(
            body["data"]["network"]["httpClients"],
            serde_json::json!([])
        );
        assert_eq!(
            body["data"]["network"]["httpSseServers"],
            serde_json::json!([])
        );
        assert_eq!(
            body["data"]["network"]["websocketServers"],
            serde_json::json!([])
        );
        assert_eq!(
            body["data"]["network"]["websocketClients"],
            serde_json::json!([])
        );
        assert_eq!(body["data"]["parseMultMsg"], true);
        assert_eq!(body["data"]["timeout"]["baseTimeout"], 10_000);
    }

    #[test]
    fn i18n_route_returns_current_catalog_snapshot() {
        let response = test_server().route_http_request(
            b"GET /api/i18n HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("i18n body should be valid json");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(body["locale"], "zh-CN");
        assert_eq!(body["fallback_locale"], "zh-CN");
        assert_eq!(
            body["messages"]["command.spec.help.summary"],
            "显示当前作用域可用命令"
        );
        assert_eq!(body["messages"]["web.nav.overview"], "总览");
        assert_eq!(body["messages"]["web.runtime.status.running"], "运行中");
    }

    #[test]
    fn static_asset_route_returns_injected_asset() {
        let response = test_server().route_http_request(
            b"GET /assets/bot.svg HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body = String::from_utf8(body).expect("body should be utf8");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("Content-Type: image/svg+xml; charset=utf-8\r\n"));
        assert_eq!(body, TEST_SVG);
    }

    #[test]
    fn head_request_returns_headers_without_body() {
        let response = test_server().route_http_request(
            b"HEAD / HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains(&format!("Content-Length: {}\r\n", TEST_HTML.len())));
        assert!(body.is_empty());
    }

    #[test]
    fn missing_asset_returns_not_found() {
        let response = test_server().route_http_request(
            b"GET /missing HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);
        let body = String::from_utf8(body).expect("body should be utf8");

        assert!(headers.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert_eq!(body, "not found");
    }

    fn temp_dir_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("rsliteyukibot-web-host-{name}-{unique}"))
    }

    #[test]
    fn directory_asset_mode_serves_files_and_spa_fallback() {
        let root = temp_dir_path("dist");
        fs::create_dir_all(root.join("assets")).expect("asset dir should be created");
        fs::write(
            root.join("index.html"),
            "<!doctype html><title>Dist</title>",
        )
        .expect("index should be written");
        fs::write(root.join("assets").join("app.js"), "console.log('ok');")
            .expect("app.js should be written");

        let assets = WebHostAssets::new(WebHostAsset::text(
            "text/html; charset=utf-8",
            "<!doctype html><title>Fallback</title>",
        ))
        .with_asset_directory(root.clone());
        let server = WebHostService {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
            browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            dev_frontend: None,
            snapshot_provider: Arc::new(AppHostSnapshot::default),
            assets: Arc::new(assets),
            terminal_state: Arc::new(WebTerminalState::default()),
        };

        let js_response = server.route_http_request(
            b"GET /assets/app.js HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (js_headers, js_body) = split_response(js_response);
        let js_body = String::from_utf8(js_body).expect("js body should be utf8");
        assert!(js_headers.contains("Content-Type: text/javascript; charset=utf-8\r\n"));
        assert_eq!(js_body, "console.log('ok');");

        let spa_response = server.route_http_request(
            b"GET /dashboard HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (spa_headers, spa_body) = split_response(spa_response);
        let spa_body = String::from_utf8(spa_body).expect("spa body should be utf8");
        assert!(spa_headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(spa_body, "<!doctype html><title>Dist</title>");

        let _ = fs::remove_file(root.join("assets").join("app.js"));
        let _ = fs::remove_file(root.join("index.html"));
        let _ = fs::remove_dir(root.join("assets"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn dev_frontend_redirects_non_api_routes_when_probe_is_alive() {
        let probe_listener =
            StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe listener should bind");
        let probe_addr = probe_listener
            .local_addr()
            .expect("probe listener should expose local addr");
        let server = WebHostService {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
            browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            dev_frontend: Some(WebHostDevServer {
                probe_addr,
                public_port: 1420,
            }),
            snapshot_provider: Arc::new(AppHostSnapshot::default),
            assets: Arc::new(test_assets()),
            terminal_state: Arc::new(WebTerminalState::default()),
        };

        let response = server.route_http_request(
            b"GET /dashboard?tab=runtime HTTP/1.1\r\nHost: 192.168.2.2:14500\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (headers, body) = split_response(response);

        assert!(headers.starts_with("HTTP/1.1 307 Temporary Redirect\r\n"));
        assert!(headers.contains("Location: http://192.168.2.2:1420/dashboard?tab=runtime\r\n"));
        assert_eq!(
            String::from_utf8(body).expect("body should be utf8"),
            "redirecting"
        );
    }

    #[test]
    fn dev_frontend_redirect_keeps_local_api_and_static_routes() {
        let probe_listener =
            StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe listener should bind");
        let probe_addr = probe_listener
            .local_addr()
            .expect("probe listener should expose local addr");
        let server = WebHostService {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
            browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            dev_frontend: Some(WebHostDevServer {
                probe_addr,
                public_port: 1420,
            }),
            snapshot_provider: Arc::new(AppHostSnapshot::default),
            assets: Arc::new(test_assets()),
            terminal_state: Arc::new(WebTerminalState::default()),
        };

        let api_response = server.route_http_request(
            b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (api_headers, _) = split_response(api_response);
        assert!(api_headers.starts_with("HTTP/1.1 200 OK\r\n"));

        let i18n_response = server.route_http_request(
            b"GET /api/i18n HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (i18n_headers, _) = split_response(i18n_response);
        assert!(i18n_headers.starts_with("HTTP/1.1 200 OK\r\n"));

        let icon_response = server.route_http_request(
            b"GET /favicon.ico HTTP/1.1\r\nHost: localhost\r\n\r\n",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let (icon_headers, _) = split_response(icon_response);
        assert!(icon_headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(icon_headers.contains("Content-Type: image/x-icon\r\n"));
    }
}
