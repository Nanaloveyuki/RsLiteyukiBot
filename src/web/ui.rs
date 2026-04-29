use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::host::{WebHostAsset, WebHostAssets, WebHostConfig, WebHostDevServer};
use crate::{AdapterConfig, AdapterEndpoint, AdapterRoute, AdapterTransport};

pub const APP_SHELL_PLACEHOLDER_HTML: &str =
    include_str!("../../assets/app-shell/placeholder.html");
pub const APP_SHELL_LOGO_SVG: &str = include_str!("../../assets/app-shell/bot.svg");
pub const APP_SHELL_WINDOW_ICON_ICO: &[u8] = include_bytes!("../../assets/app-shell/bot.ico");
pub const FRONTEND_DIST_DIR: &str = "frontend/dist";
pub const FRONTEND_LOGO_ASSET_PATH: &str = "/assets/bot.svg";
pub const FRONTEND_FAVICON_ASSET_PATH: &str = "/favicon.ico";
pub const WEB_DEV_SERVER_ENV: &str = "LY_WEB_DEV_SERVER";

const DEFAULT_MESSAGE_POST_FORMAT: &str = "array";
const DEFAULT_FILE_TRANSFER_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_FILE_TRANSFER_SPEED_KBPS: u64 = 256;
const DEFAULT_MAX_TIMEOUT_MS: u64 = 1_800_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NapCatWebUIConfig {
    pub host: String,
    pub port: u16,
    pub token: String,
    #[serde(rename = "loginRate")]
    pub login_rate: u32,
    #[serde(rename = "disableWebUI")]
    pub disable_webui: bool,
    #[serde(rename = "accessControlMode")]
    pub access_control_mode: String,
    #[serde(rename = "ipWhitelist")]
    pub ip_whitelist: Vec<String>,
    #[serde(rename = "ipBlacklist")]
    pub ip_blacklist: Vec<String>,
    #[serde(rename = "enableXForwardedFor")]
    pub enable_x_forwarded_for: bool,
}

impl Default for NapCatWebUIConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 0,
            token: String::new(),
            login_rate: 10,
            disable_webui: false,
            access_control_mode: "none".to_string(),
            ip_whitelist: Vec::new(),
            ip_blacklist: Vec::new(),
            enable_x_forwarded_for: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NapCatBypassConfig {
    pub hook: bool,
    pub window: bool,
    pub module: bool,
    pub process: bool,
    pub container: bool,
    pub js: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NapCatConfig {
    #[serde(rename = "fileLog")]
    pub file_log: bool,
    #[serde(rename = "consoleLog")]
    pub console_log: bool,
    #[serde(rename = "fileLogLevel")]
    pub file_log_level: String,
    #[serde(rename = "consoleLogLevel")]
    pub console_log_level: String,
    #[serde(rename = "packetBackend")]
    pub packet_backend: String,
    #[serde(rename = "packetServer")]
    pub packet_server: String,
    #[serde(rename = "o3HookMode")]
    pub o3_hook_mode: u8,
    #[serde(rename = "autoTimeSync")]
    pub auto_time_sync: bool,
    pub bypass: NapCatBypassConfig,
}

impl Default for NapCatConfig {
    fn default() -> Self {
        Self {
            file_log: false,
            console_log: true,
            file_log_level: "debug".to_string(),
            console_log_level: "info".to_string(),
            packet_backend: "auto".to_string(),
            packet_server: String::new(),
            o3_hook_mode: 0,
            auto_time_sync: true,
            bypass: NapCatBypassConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OneBotTimeoutConfig {
    #[serde(rename = "baseTimeout")]
    pub base_timeout: u64,
    #[serde(rename = "uploadSpeedKBps")]
    pub upload_speed_kbps: u64,
    #[serde(rename = "downloadSpeedKBps")]
    pub download_speed_kbps: u64,
    #[serde(rename = "maxTimeout")]
    pub max_timeout: u64,
}

impl Default for OneBotTimeoutConfig {
    fn default() -> Self {
        Self {
            base_timeout: DEFAULT_FILE_TRANSFER_TIMEOUT_MS,
            upload_speed_kbps: DEFAULT_FILE_TRANSFER_SPEED_KBPS,
            download_speed_kbps: DEFAULT_FILE_TRANSFER_SPEED_KBPS,
            max_timeout: DEFAULT_MAX_TIMEOUT_MS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OneBotAdapterBase {
    pub name: String,
    pub enable: bool,
    pub debug: bool,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OneBotHttpServerConfig {
    #[serde(flatten)]
    pub base: OneBotAdapterBase,
    pub port: u16,
    pub host: String,
    #[serde(rename = "enableCors")]
    pub enable_cors: bool,
    #[serde(rename = "enableWebsocket")]
    pub enable_websocket: bool,
    #[serde(rename = "messagePostFormat")]
    pub message_post_format: String,
}

impl Default for OneBotHttpServerConfig {
    fn default() -> Self {
        Self {
            base: OneBotAdapterBase::default(),
            port: 3000,
            host: "0.0.0.0".to_string(),
            enable_cors: false,
            enable_websocket: false,
            message_post_format: DEFAULT_MESSAGE_POST_FORMAT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OneBotHttpClientConfig {
    #[serde(flatten)]
    pub base: OneBotAdapterBase,
    pub url: String,
    #[serde(rename = "messagePostFormat")]
    pub message_post_format: String,
    #[serde(rename = "reportSelfMessage")]
    pub report_self_message: bool,
}

impl Default for OneBotHttpClientConfig {
    fn default() -> Self {
        Self {
            base: OneBotAdapterBase::default(),
            url: String::new(),
            message_post_format: DEFAULT_MESSAGE_POST_FORMAT.to_string(),
            report_self_message: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OneBotHttpSseServerConfig {
    #[serde(flatten)]
    pub server: OneBotHttpServerConfig,
    #[serde(rename = "reportSelfMessage")]
    pub report_self_message: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OneBotWebsocketServerConfig {
    #[serde(flatten)]
    pub base: OneBotAdapterBase,
    pub host: String,
    pub port: u16,
    #[serde(rename = "messagePostFormat")]
    pub message_post_format: String,
    #[serde(rename = "reportSelfMessage")]
    pub report_self_message: bool,
    #[serde(rename = "enableForcePushEvent")]
    pub enable_force_push_event: bool,
    #[serde(rename = "heartInterval")]
    pub heart_interval: u64,
}

impl Default for OneBotWebsocketServerConfig {
    fn default() -> Self {
        Self {
            base: OneBotAdapterBase::default(),
            host: "0.0.0.0".to_string(),
            port: 3001,
            message_post_format: DEFAULT_MESSAGE_POST_FORMAT.to_string(),
            report_self_message: false,
            enable_force_push_event: false,
            heart_interval: 30_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OneBotWebsocketClientConfig {
    #[serde(flatten)]
    pub base: OneBotAdapterBase,
    pub url: String,
    #[serde(rename = "messagePostFormat")]
    pub message_post_format: String,
    #[serde(rename = "reportSelfMessage")]
    pub report_self_message: bool,
    #[serde(rename = "reconnectInterval")]
    pub reconnect_interval: u64,
    #[serde(rename = "heartInterval")]
    pub heart_interval: u64,
}

impl Default for OneBotWebsocketClientConfig {
    fn default() -> Self {
        Self {
            base: OneBotAdapterBase::default(),
            url: String::new(),
            message_post_format: DEFAULT_MESSAGE_POST_FORMAT.to_string(),
            report_self_message: false,
            reconnect_interval: 5_000,
            heart_interval: 30_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OneBotNetworkConfig {
    #[serde(rename = "httpServers")]
    pub http_servers: Vec<OneBotHttpServerConfig>,
    #[serde(rename = "httpClients")]
    pub http_clients: Vec<OneBotHttpClientConfig>,
    #[serde(rename = "httpSseServers")]
    pub http_sse_servers: Vec<OneBotHttpSseServerConfig>,
    #[serde(rename = "websocketServers")]
    pub websocket_servers: Vec<OneBotWebsocketServerConfig>,
    #[serde(rename = "websocketClients")]
    pub websocket_clients: Vec<OneBotWebsocketClientConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OneBotConfig {
    pub network: OneBotNetworkConfig,
    #[serde(rename = "musicSignUrl")]
    pub music_sign_url: String,
    #[serde(rename = "enableLocalFile2Url")]
    pub enable_local_file2url: bool,
    #[serde(rename = "parseMultMsg")]
    pub parse_mult_msg: bool,
    #[serde(rename = "imageDownloadProxy")]
    pub image_download_proxy: String,
    pub timeout: OneBotTimeoutConfig,
}

impl Default for OneBotConfig {
    fn default() -> Self {
        Self {
            network: OneBotNetworkConfig::default(),
            music_sign_url: String::new(),
            enable_local_file2url: false,
            parse_mult_msg: true,
            image_download_proxy: String::new(),
            timeout: OneBotTimeoutConfig::default(),
        }
    }
}

impl OneBotConfig {
    pub fn to_runtime_adapter_configs(&self) -> Result<Vec<AdapterConfig>, String> {
        let mut adapters = Vec::new();

        for config in &self.network.http_servers {
            if config.base.enable {
                return Err(format!(
                    "network.httpServers '{}' is not supported by the current runtime",
                    config.base.name
                ));
            }
        }

        for config in &self.network.http_sse_servers {
            if !config.server.base.enable {
                continue;
            }
            adapters.push(AdapterConfig {
                id: config.server.base.name.clone(),
                enabled: true,
                transport: AdapterTransport::Sse,
                endpoint: AdapterEndpoint {
                    url: format_http_bind_url(
                        config.server.host.as_str(),
                        config.server.port,
                        "/sse",
                    ),
                    headers: Default::default(),
                    token: non_empty_token(config.server.base.token.as_str()),
                    timeout_ms: self.timeout.base_timeout.max(10),
                },
                route: AdapterRoute::default(),
                queue_capacity: 256,
                max_payload_size: None,
                max_connections: None,
            });
        }

        for config in &self.network.http_clients {
            if !config.base.enable {
                continue;
            }
            adapters.push(AdapterConfig {
                id: config.base.name.clone(),
                enabled: true,
                transport: AdapterTransport::Http,
                endpoint: AdapterEndpoint {
                    url: config.url.clone(),
                    headers: Default::default(),
                    token: non_empty_token(config.base.token.as_str()),
                    timeout_ms: self.timeout.base_timeout.max(10),
                },
                route: AdapterRoute::default(),
                queue_capacity: 256,
                max_payload_size: None,
                max_connections: None,
            });
        }

        for config in &self.network.websocket_clients {
            if !config.base.enable {
                continue;
            }
            adapters.push(AdapterConfig {
                id: config.base.name.clone(),
                enabled: true,
                transport: AdapterTransport::WebSocketForward,
                endpoint: AdapterEndpoint {
                    url: config.url.clone(),
                    headers: Default::default(),
                    token: non_empty_token(config.base.token.as_str()),
                    timeout_ms: self.timeout.base_timeout.max(10),
                },
                route: AdapterRoute::default(),
                queue_capacity: 256,
                max_payload_size: None,
                max_connections: None,
            });
        }

        for config in &self.network.websocket_servers {
            if !config.base.enable {
                continue;
            }
            adapters.push(AdapterConfig {
                id: config.base.name.clone(),
                enabled: true,
                transport: AdapterTransport::WebSocketReverse,
                endpoint: AdapterEndpoint {
                    url: format_websocket_bind_url(config.host.as_str(), config.port),
                    headers: Default::default(),
                    token: non_empty_token(config.base.token.as_str()),
                    timeout_ms: self.timeout.base_timeout.max(10),
                },
                route: AdapterRoute::default(),
                queue_capacity: 256,
                max_payload_size: None,
                max_connections: None,
            });
        }

        adapters.sort_by(|left, right| left.id.cmp(&right.id));
        for adapter in &adapters {
            adapter
                .validate()
                .map_err(|err| format!("adapter '{}' is invalid: {err}", adapter.id))?;
        }

        Ok(adapters)
    }
}

fn non_empty_token(raw: &str) -> Option<String> {
    let token = raw.trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn format_websocket_bind_url(host: &str, port: u16) -> String {
    format!("ws://{}:{}/", host.trim(), port)
}

fn format_http_bind_url(host: &str, port: u16, path: &str) -> String {
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    format!("http://{}:{}{}", host.trim(), port, normalized_path)
}

pub fn build_default_web_host_assets() -> WebHostAssets {
    build_web_host_assets(resolve_frontend_dist_dir())
}

pub fn build_web_host_assets(dist_dir: Option<PathBuf>) -> WebHostAssets {
    let index_asset = dist_dir
        .as_ref()
        .and_then(|dir| fs::read_to_string(dir.join("index.html")).ok())
        .map(|html| WebHostAsset::text("text/html; charset=utf-8", html))
        .unwrap_or_else(|| {
            WebHostAsset::text("text/html; charset=utf-8", APP_SHELL_PLACEHOLDER_HTML)
        });

    let assets = WebHostAssets::new(index_asset)
        .with_asset(
            FRONTEND_LOGO_ASSET_PATH,
            WebHostAsset::text("image/svg+xml; charset=utf-8", APP_SHELL_LOGO_SVG),
        )
        .with_asset(
            FRONTEND_FAVICON_ASSET_PATH,
            WebHostAsset::binary("image/x-icon", APP_SHELL_WINDOW_ICON_ICO),
        );

    if let Some(dist_dir) = dist_dir {
        assets.with_asset_directory(dist_dir)
    } else {
        assets
    }
}

pub fn build_default_web_host_config() -> WebHostConfig {
    WebHostConfig {
        dev_frontend: resolve_dev_frontend_from_env(),
        ..WebHostConfig::default()
    }
}

pub fn resolve_dev_frontend_from_env() -> Option<WebHostDevServer> {
    let raw = std::env::var(WEB_DEV_SERVER_ENV).ok()?;
    let probe_addr = raw.trim().parse().ok()?;
    Some(WebHostDevServer {
        probe_addr,
        public_port: probe_addr.port(),
    })
}

pub fn resolve_frontend_dist_dir() -> Option<PathBuf> {
    let current_dir = std::env::current_dir().ok();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let mut candidates = Vec::new();

    if let Some(dir) = current_dir.as_ref() {
        candidates.push(dir.join(FRONTEND_DIST_DIR));
        candidates.push(dir.join("..").join(FRONTEND_DIST_DIR));
    }

    candidates.push(manifest_dir.join(FRONTEND_DIST_DIR));

    if let Some(dir) = exe_dir.as_deref() {
        candidates.extend(packaged_frontend_dist_dir_candidates(dir));
    }

    resolve_frontend_dist_dir_from_candidates(candidates.into_iter().map(Some))
}

fn packaged_frontend_dist_dir_candidates(exe_dir: &Path) -> Vec<PathBuf> {
    vec![
        exe_dir.join(FRONTEND_DIST_DIR),
        exe_dir.join("resources").join(FRONTEND_DIST_DIR),
        exe_dir.join("..").join("Resources").join(FRONTEND_DIST_DIR),
        exe_dir
            .join("..")
            .join("Resources")
            .join("resources")
            .join(FRONTEND_DIST_DIR),
    ]
}

fn resolve_frontend_dist_dir_from_candidates(
    candidates: impl IntoIterator<Item = Option<PathBuf>>,
) -> Option<PathBuf> {
    candidates
        .into_iter()
        .flatten()
        .map(normalize_path)
        .find(|dir| dir.join("index.html").is_file())
}

fn normalize_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(test)]
#[path = "ui/tests.rs"]
mod tests;
