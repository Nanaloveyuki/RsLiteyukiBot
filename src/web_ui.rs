use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::web_host::{WebHostAsset, WebHostAssets, WebHostConfig, WebHostDevServer};

pub const APP_SHELL_PLACEHOLDER_HTML: &str = include_str!("../assets/app-shell/placeholder.html");
pub const APP_SHELL_LOGO_SVG: &str = include_str!("../assets/app-shell/bot.svg");
pub const APP_SHELL_WINDOW_ICON_ICO: &[u8] = include_bytes!("../assets/app-shell/bot.ico");
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
}

impl Default for NapCatWebUIConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 0,
            token: String::new(),
            login_rate: 3,
        }
    }
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
}

impl Default for NapCatConfig {
    fn default() -> Self {
        Self {
            file_log: false,
            console_log: true,
            file_log_level: "debug".to_string(),
            console_log_level: "info".to_string(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OneBotHttpSseServerConfig {
    #[serde(flatten)]
    pub server: OneBotHttpServerConfig,
    #[serde(rename = "reportSelfMessage")]
    pub report_self_message: bool,
}

impl Default for OneBotHttpSseServerConfig {
    fn default() -> Self {
        Self {
            server: OneBotHttpServerConfig::default(),
            report_self_message: false,
        }
    }
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
    let mut config = WebHostConfig::default();
    config.dev_frontend = resolve_dev_frontend_from_env();
    config
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
    resolve_frontend_dist_dir_from_candidates([
        current_dir.as_ref().map(|dir| dir.join(FRONTEND_DIST_DIR)),
        current_dir
            .as_ref()
            .map(|dir| dir.join("..").join(FRONTEND_DIST_DIR)),
        Some(manifest_dir.join(FRONTEND_DIST_DIR)),
    ])
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
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_dir_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("rsliteyukibot-web-ui-{name}-{unique}"))
    }

    #[test]
    fn resolve_frontend_dist_dir_uses_first_candidate_with_index_html() {
        let missing_root = temp_dir_path("missing");
        let valid_root = temp_dir_path("valid");

        fs::create_dir_all(&missing_root).expect("missing candidate dir should exist");
        fs::create_dir_all(&valid_root).expect("valid candidate dir should exist");
        fs::write(valid_root.join("index.html"), "<!doctype html>")
            .expect("index.html should be written");

        let resolved = resolve_frontend_dist_dir_from_candidates([
            Some(missing_root.clone()),
            Some(valid_root.clone()),
        ]);

        assert_eq!(resolved, Some(normalize_path(valid_root.clone())));

        let _ = fs::remove_file(valid_root.join("index.html"));
        let _ = fs::remove_dir(&missing_root);
        let _ = fs::remove_dir(&valid_root);
    }

    #[test]
    fn resolve_frontend_dist_dir_returns_none_without_index_html() {
        let missing_root = temp_dir_path("none");
        fs::create_dir_all(&missing_root).expect("candidate dir should exist");

        let resolved = resolve_frontend_dist_dir_from_candidates([Some(missing_root.clone())]);

        assert!(resolved.is_none());

        let _ = fs::remove_dir(&missing_root);
    }

    #[test]
    fn resolve_dev_frontend_reads_probe_addr_from_env() {
        let _lock = env_lock().lock().expect("env lock should not be poisoned");
        unsafe {
            std::env::set_var(WEB_DEV_SERVER_ENV, "127.0.0.1:1420");
        }

        let dev_frontend = resolve_dev_frontend_from_env();

        unsafe {
            std::env::remove_var(WEB_DEV_SERVER_ENV);
        }

        assert_eq!(
            dev_frontend,
            Some(WebHostDevServer {
                probe_addr: "127.0.0.1:1420"
                    .parse()
                    .expect("socket addr should parse"),
                public_port: 1420,
            })
        );
    }

    #[test]
    fn onebot_config_defaults_match_napcat_dashboard_shape() {
        let config = OneBotConfig::default();
        let json = serde_json::to_value(&config).expect("config should serialize");

        assert_eq!(json["network"]["httpServers"], serde_json::json!([]));
        assert_eq!(json["network"]["httpClients"], serde_json::json!([]));
        assert_eq!(json["network"]["httpSseServers"], serde_json::json!([]));
        assert_eq!(json["network"]["websocketServers"], serde_json::json!([]));
        assert_eq!(json["network"]["websocketClients"], serde_json::json!([]));
        assert_eq!(json["parseMultMsg"], true);
        assert_eq!(json["timeout"]["baseTimeout"], DEFAULT_FILE_TRANSFER_TIMEOUT_MS);
        assert_eq!(
            json["timeout"]["uploadSpeedKBps"],
            DEFAULT_FILE_TRANSFER_SPEED_KBPS
        );
    }
}
