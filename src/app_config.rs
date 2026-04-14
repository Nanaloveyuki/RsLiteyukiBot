use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use liteyukibot_core::AdapterConfig;
use serde::Deserialize;

use crate::tui;

pub(crate) const APP_CONFIG_PATHS: [&str; 6] = [
    "config.yaml",
    "rust-config.yaml",
    "rust-config.yml",
    "rust-config.toml",
    "config/rust-core.yaml",
    "config/rust-core.toml",
];

static LAST_RELOAD_WARNING_STATE: LazyLock<Mutex<Option<ReloadWarningState>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AppConfigDoc {
    #[serde(default)]
    pub(crate) rust: Option<AppRustSection>,
    #[serde(default)]
    pub(crate) runtime: Option<RuntimeConfigSection>,
    #[serde(default)]
    pub(crate) log: Option<LogConfigSection>,
    #[serde(default)]
    pub(crate) adapters: Option<Vec<AdapterConfig>>,
    #[serde(default)]
    pub(crate) connect: Option<ConnectConfigSection>,
    #[serde(default)]
    pub(crate) tui: Option<TuiConfigSection>,
    #[serde(default, rename = "onebot-v11", alias = "onebot_v11")]
    pub(crate) onebot_v11: Option<OnebotV11ConfigSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct OnebotV11ConfigSection {
    #[serde(default)]
    pub(crate) whitelist: Vec<OnebotWhitelistEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum OnebotWhitelistEntry {
    Text(String),
    Int(i64),
    UInt(u64),
}

impl OnebotWhitelistEntry {
    fn as_token(&self) -> String {
        match self {
            Self::Text(raw) => raw.trim().to_string(),
            Self::Int(raw) => raw.to_string(),
            Self::UInt(raw) => raw.to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AppRustSection {
    #[serde(default)]
    pub(crate) runtime: Option<RuntimeConfigSection>,
    #[serde(default)]
    pub(crate) log: Option<LogConfigSection>,
    #[serde(default)]
    pub(crate) adapters: Option<Vec<AdapterConfig>>,
    #[serde(default)]
    pub(crate) tui: Option<TuiConfigSection>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct RuntimeConfigSection {
    #[serde(default)]
    pub(crate) worker_count: Option<usize>,
    #[serde(default)]
    pub(crate) ingress_queue: Option<usize>,
    #[serde(default)]
    pub(crate) worker_queue: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct LogConfigSection {
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) level: Option<String>,
    #[serde(default)]
    pub(crate) timezone: Option<String>,
    #[serde(default)]
    pub(crate) timestamp_format: Option<String>,
    #[serde(default)]
    pub(crate) timestamp_pattern: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct ConnectConfigSection {
    #[serde(default)]
    pub(crate) websocket: Option<WebSocketConnectSection>,
    #[serde(default, rename = "tcp-http")]
    pub(crate) tcp_http: Option<HttpConnectSection>,
    #[serde(default)]
    pub(crate) sse: Option<SseConnectSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct WebSocketConnectSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) queue_capacity: Option<usize>,
    #[serde(default)]
    pub(crate) max_payload_size: Option<usize>,
    #[serde(default)]
    pub(crate) max_connections: Option<usize>,
    #[serde(default)]
    pub(crate) inbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) outbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) forward: Option<WebSocketEndpointSection>,
    #[serde(default)]
    pub(crate) reverse: Option<WebSocketEndpointSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct WebSocketEndpointSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) queue_capacity: Option<usize>,
    #[serde(default)]
    pub(crate) max_payload_size: Option<usize>,
    #[serde(default)]
    pub(crate) max_connections: Option<usize>,
    #[serde(default)]
    pub(crate) inbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct HttpConnectSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) queue_capacity: Option<usize>,
    #[serde(default)]
    pub(crate) max_payload_size: Option<usize>,
    #[serde(default)]
    pub(crate) max_connections: Option<usize>,
    #[serde(default)]
    pub(crate) inbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SseConnectSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) queue_capacity: Option<usize>,
    #[serde(default)]
    pub(crate) max_payload_size: Option<usize>,
    #[serde(default)]
    pub(crate) max_connections: Option<usize>,
    #[serde(default)]
    pub(crate) inbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct TuiConfigSection {
    #[serde(default)]
    pub(crate) resume: Option<TuiResumeSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct TuiResumeSection {
    #[serde(default)]
    pub(crate) store_path: Option<String>,
    #[serde(default)]
    pub(crate) max_sessions: Option<usize>,
    #[serde(default)]
    pub(crate) max_size_mib: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdapterConfigDoc {
    pub(crate) adapters: Vec<AdapterConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ReloadWarningState {
    runtime: Option<RuntimeConfigSection>,
    log: Option<LogConfigSection>,
}

impl ReloadWarningState {
    pub(crate) fn from_doc(doc: &AppConfigDoc) -> Self {
        Self {
            runtime: config_runtime(doc).cloned(),
            log: config_log(doc).cloned(),
        }
    }
}

pub(crate) fn load_app_config() -> AppConfigDoc {
    load_app_config_with_warnings(true).0
}

pub(crate) fn load_app_config_with_warnings(emit_stderr: bool) -> (AppConfigDoc, Vec<String>) {
    let Some(path) = resolve_app_config_path() else {
        return (AppConfigDoc::default(), Vec::new());
    };

    match load_app_config_from_path(&path) {
        Ok(doc) => {
            let warnings = validate_app_config(&doc);
            if emit_stderr {
                for warning in &warnings {
                    eprintln!("config warning ({}): {warning}", path.display());
                }
            }
            (doc, warnings)
        }
        Err(err) => {
            let message = format!("failed to load app config from {}: {err}", path.display());
            if emit_stderr {
                eprintln!("{message}");
            }
            (AppConfigDoc::default(), vec![message])
        }
    }
}

pub(crate) fn resolve_app_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("LY_CONFIG_PATH") {
        return Some(PathBuf::from(path));
    }

    APP_CONFIG_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

pub(crate) fn ensure_default_config_files() -> Result<(), Box<dyn std::error::Error>> {
    write_default_config_if_missing(Path::new("config.yaml"))?;

    if let Ok(path) = std::env::var("LY_CONFIG_PATH") {
        let path = PathBuf::from(path);
        write_default_config_if_missing(path.as_path())?;
    }

    Ok(())
}

pub(crate) fn write_default_config_if_missing(
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let template = default_config_template(path);
    std::fs::write(path, template)?;
    eprintln!("created default config file: {}", path.display());
    Ok(())
}

fn default_config_template(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match ext.as_deref() {
        Some("toml") => DEFAULT_TOML_CONFIG_TEMPLATE.to_string(),
        _ => DEFAULT_YAML_CONFIG_TEMPLATE.to_string(),
    }
}

const DEFAULT_YAML_CONFIG_TEMPLATE: &str = r#"rust:
  runtime:
    worker_count: 4
    ingress_queue: 1024
    worker_queue: 256
  log:
    mode: color
    level: info
    timezone: local
    timestamp_format: custom
    timestamp_pattern: "%Y-%m-%d %H:%M:%S"
  adapters: []
  tui:
    resume:
      store_path: ./.liteyuki-tui-resumes.json
      max_sessions: 64
      max_size_mib: 16

connect:
  websocket:
    enabled: true
    # mode: forward | reverse | both
    mode: reverse
    # reverse mode can use port (+ optional host/path)
    host: 0.0.0.0
    port: 8080
    path: /ws
    # forward mode can use url directly
    # url: ws://127.0.0.1:3000/ws
    max_payload_size: 1048576
    max_connections: 100
    timeout_seconds: 30
  tcp-http:
    enabled: true
    host: 127.0.0.1
    port: 8081
    path: /
    max_payload_size: 1048576
    max_connections: 100
    timeout_seconds: 30
  sse:
    enabled: true
    host: 127.0.0.1
    port: 8082
    path: /sse
    max_payload_size: 1048576
    max_connections: 100
    timeout_seconds: 30

onebot-v11:
  # 仅白名单会话可触发外部 /help。
  # 可写纯ID（private常用 user_id；group常用 group_id）或带前缀:
  # - "3541766758"
  # - "private:3541766758"
  # - "group:699493240"
  whitelist: []
"#;

const DEFAULT_TOML_CONFIG_TEMPLATE: &str = r#"[rust]
adapters = []

[rust.runtime]
worker_count = 4
ingress_queue = 1024
worker_queue = 256

[rust.log]
mode = "color"
level = "info"
timezone = "local"
timestamp_format = "custom"
timestamp_pattern = "%Y-%m-%d %H:%M:%S"

[rust.tui.resume]
store_path = "./.liteyuki-tui-resumes.json"
max_sessions = 64
max_size_mib = 16

[connect.websocket]
enabled = true
mode = "reverse" # forward | reverse | both
host = "0.0.0.0"
port = 8080
path = "/ws"
max_payload_size = 1048576
max_connections = 100
timeout_seconds = 30

[connect.tcp-http]
enabled = true
host = "127.0.0.1"
port = 8081
path = "/"
max_payload_size = 1048576
max_connections = 100
timeout_seconds = 30

[connect.sse]
enabled = true
host = "127.0.0.1"
port = 8082
path = "/sse"
max_payload_size = 1048576
max_connections = 100
timeout_seconds = 30

[onebot-v11]
# whitelist = ["3541766758", "private:3541766758", "group:699493240"]
whitelist = []
"#;

pub(crate) fn load_app_config_from_path(
    path: &Path,
) -> Result<AppConfigDoc, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match ext.as_deref() {
        Some("yaml") | Some("yml") => Ok(serde_yaml::from_str::<AppConfigDoc>(&content)?),
        Some("toml") => Ok(toml::from_str::<AppConfigDoc>(&content)?),
        _ => Err(format!("unsupported config extension for {}", path.display()).into()),
    }
}

fn config_adapters(doc: &AppConfigDoc) -> Option<&Vec<AdapterConfig>> {
    doc.rust
        .as_ref()
        .and_then(|section| section.adapters.as_ref())
        .or(doc.adapters.as_ref())
}

fn config_connect(doc: &AppConfigDoc) -> Option<&ConnectConfigSection> {
    doc.connect.as_ref()
}

fn config_tui_resume(doc: &AppConfigDoc) -> Option<&TuiResumeSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.tui.as_ref())
        .and_then(|tui| tui.resume.as_ref())
        .or(doc.tui.as_ref().and_then(|tui| tui.resume.as_ref()))
}

fn config_runtime(doc: &AppConfigDoc) -> Option<&RuntimeConfigSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.runtime.as_ref())
        .or(doc.runtime.as_ref())
}

fn config_log(doc: &AppConfigDoc) -> Option<&LogConfigSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.log.as_ref())
        .or(doc.log.as_ref())
}

fn config_onebot_v11(doc: &AppConfigDoc) -> Option<&OnebotV11ConfigSection> {
    doc.onebot_v11.as_ref()
}

pub(crate) fn resolve_help_whitelist(doc: &AppConfigDoc) -> HashSet<String> {
    config_onebot_v11(doc)
        .map(|section| {
            section
                .whitelist
                .iter()
                .map(OnebotWhitelistEntry::as_token)
                .filter(|value| !value.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn load_adapter_configs(
    app_config: &AppConfigDoc,
) -> Result<Vec<AdapterConfig>, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("LY_ADAPTERS_PATH") {
        let content = std::fs::read_to_string(PathBuf::from(path))?;
        if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(&content) {
            return Ok(sanitize_adapter_configs(doc.adapters, "LY_ADAPTERS_PATH"));
        }
        let list = serde_json::from_str::<Vec<AdapterConfig>>(&content)?;
        return Ok(sanitize_adapter_configs(list, "LY_ADAPTERS_PATH"));
    }

    if let Ok(raw) = std::env::var("LY_ADAPTERS_JSON") {
        if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(&raw) {
            return Ok(sanitize_adapter_configs(doc.adapters, "LY_ADAPTERS_JSON"));
        }
        let list = serde_json::from_str::<Vec<AdapterConfig>>(&raw)?;
        return Ok(sanitize_adapter_configs(list, "LY_ADAPTERS_JSON"));
    }

    let mut combined = config_adapters(app_config).cloned().unwrap_or_default();
    combined.extend(connect_to_adapter_configs(app_config));
    Ok(sanitize_adapter_configs(combined, "config"))
}

pub(crate) fn resolve_tui_config(app_config: &AppConfigDoc) -> tui::TuiConfig {
    let mut config = tui::TuiConfig::default();

    if let Some(resume) = config_tui_resume(app_config) {
        if let Some(path) = resume.store_path.as_deref() {
            config.resume_store_path = PathBuf::from(path);
        }
        if let Some(max_sessions) = resume.max_sessions
            && max_sessions > 0
        {
            config.resume_max_sessions = max_sessions;
        }
        if let Some(max_size_mib) = resume.max_size_mib
            && max_size_mib > 0
        {
            config.resume_max_size_mib = max_size_mib;
        }
    }

    if let Ok(path) = std::env::var("LY_RESUME_STORE_PATH") {
        config.resume_store_path = PathBuf::from(path);
    }
    if let Ok(raw) = std::env::var("LY_TUI_RESUME_MAX_SESSIONS")
        && let Ok(value) = raw.trim().parse::<usize>()
        && value > 0
    {
        config.resume_max_sessions = value;
    }
    if let Ok(raw) = std::env::var("LY_TUI_RESUME_MAX_SIZE_MIB")
        && let Ok(value) = raw.trim().parse::<u64>()
        && value > 0
    {
        config.resume_max_size_mib = value;
    }

    config
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
            if forward.enabled.unwrap_or(false)
                && let Some(config) =
                    websocket_endpoint_to_adapter("connect-ws-forward", true, forward, ws)
            {
                adapters.push(config);
            }
        }
        if let Some(reverse) = &ws.reverse {
            has_nested = true;
            if reverse.enabled.unwrap_or(false)
                && let Some(config) =
                    websocket_endpoint_to_adapter("connect-ws-reverse", false, reverse, ws)
            {
                adapters.push(config);
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

            if matches!(resolved_mode.as_str(), "forward" | "both" | "all")
                && let Some(config) = websocket_root_to_adapter("connect-ws-forward", true, ws)
            {
                adapters.push(config);
            }
            if matches!(resolved_mode.as_str(), "reverse" | "both" | "all")
                && let Some(config) = websocket_root_to_adapter("connect-ws-reverse", false, ws)
            {
                adapters.push(config);
            }
        }
    }

    if let Some(http) = &connect.tcp_http
        && http.enabled.unwrap_or(false)
    {
        adapters.push(http_to_adapter("connect-http", http));
    }

    if let Some(sse) = &connect.sse
        && sse.enabled.unwrap_or(false)
    {
        adapters.push(sse_to_adapter("connect-sse", sse));
    }

    adapters
}

fn websocket_endpoint_to_adapter(
    id: &str,
    is_forward: bool,
    endpoint: &WebSocketEndpointSection,
    fallback: &WebSocketConnectSection,
) -> Option<AdapterConfig> {
    use liteyukibot_core::{AdapterEndpoint, AdapterRoute, AdapterTransport};

    let url = endpoint.url.clone().or_else(|| {
        build_url(
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

    let url = url?;

    Some(AdapterConfig {
        id: id.to_string(),
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
            timeout_ms: seconds_to_timeout_ms(
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
}

fn websocket_root_to_adapter(
    id: &str,
    is_forward: bool,
    ws: &WebSocketConnectSection,
) -> Option<AdapterConfig> {
    let endpoint = WebSocketEndpointSection {
        enabled: Some(true),
        url: ws.url.clone(),
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
    websocket_endpoint_to_adapter(id, is_forward, &endpoint, ws)
}

fn http_to_adapter(id: &str, section: &HttpConnectSection) -> AdapterConfig {
    use liteyukibot_core::{AdapterEndpoint, AdapterRoute, AdapterTransport};

    AdapterConfig {
        id: id.to_string(),
        enabled: true,
        transport: AdapterTransport::Http,
        endpoint: AdapterEndpoint {
            url: section.url.clone().unwrap_or_else(|| {
                build_url(
                    "http",
                    section.host.as_deref().unwrap_or("127.0.0.1"),
                    section.port,
                    section.path.as_deref().unwrap_or("/"),
                )
                .unwrap_or_else(|| "http://127.0.0.1:8081/".to_string())
            }),
            headers: section.headers.clone().unwrap_or_default(),
            token: section.token.clone(),
            timeout_ms: seconds_to_timeout_ms(section.timeout_seconds),
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
    }
}

fn sse_to_adapter(id: &str, section: &SseConnectSection) -> AdapterConfig {
    use liteyukibot_core::{AdapterEndpoint, AdapterRoute, AdapterTransport};

    AdapterConfig {
        id: id.to_string(),
        enabled: true,
        transport: AdapterTransport::Sse,
        endpoint: AdapterEndpoint {
            url: section.url.clone().unwrap_or_else(|| {
                build_url(
                    "http",
                    section.host.as_deref().unwrap_or("127.0.0.1"),
                    section.port,
                    section.path.as_deref().unwrap_or("/sse"),
                )
                .unwrap_or_else(|| "http://127.0.0.1:8082/sse".to_string())
            }),
            headers: section.headers.clone().unwrap_or_default(),
            token: section.token.clone(),
            timeout_ms: seconds_to_timeout_ms(section.timeout_seconds),
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
    }
}

pub(crate) fn build_url(scheme: &str, host: &str, port: Option<u16>, path: &str) -> Option<String> {
    let port = port?;
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Some(format!("{scheme}://{host}:{port}{normalized_path}"))
}

pub(crate) fn seconds_to_timeout_ms(seconds: Option<u64>) -> u64 {
    seconds.unwrap_or(5).saturating_mul(1000).max(10)
}

pub(crate) fn sanitize_adapter_configs(
    configs: Vec<AdapterConfig>,
    source: &str,
) -> Vec<AdapterConfig> {
    let mut sanitized = Vec::new();
    let mut seen = HashSet::new();
    for config in configs {
        if let Err(err) = config.validate() {
            eprintln!(
                "config warning ({}): skip invalid adapter '{}': {}",
                source, config.id, err
            );
            continue;
        }
        if !seen.insert(config.id.clone()) {
            eprintln!(
                "config warning ({}): skip duplicated adapter id '{}'",
                source, config.id
            );
            continue;
        }
        sanitized.push(config);
    }
    sanitized
}

pub(crate) fn validate_app_config(doc: &AppConfigDoc) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(adapters) = config_adapters(doc) {
        let mut seen = HashSet::new();
        for adapter in adapters {
            if let Err(err) = adapter.validate() {
                warnings.push(format!("invalid adapter '{}': {}", adapter.id, err));
            }
            if !seen.insert(adapter.id.clone()) {
                warnings.push(format!("duplicated adapter id '{}'", adapter.id));
            }
        }
    }

    if let Some(resume) = config_tui_resume(doc) {
        if let Some(path) = resume.store_path.as_deref()
            && path.trim().is_empty()
        {
            warnings.push("tui.resume.store_path should not be empty".to_string());
        }
        if let Some(max_sessions) = resume.max_sessions
            && max_sessions == 0
        {
            warnings.push("tui.resume.max_sessions should be > 0".to_string());
        }
        if let Some(max_size_mib) = resume.max_size_mib
            && max_size_mib == 0
        {
            warnings.push("tui.resume.max_size_mib should be > 0".to_string());
        }
    }

    if let Some(connect) = config_connect(doc) {
        if let Some(ws) = &connect.websocket {
            if ws.max_payload_size.is_some_and(|value| value == 0) {
                warnings.push("connect.websocket.max_payload_size should be > 0".to_string());
            }
            if ws.max_connections.is_some_and(|value| value == 0) {
                warnings.push("connect.websocket.max_connections should be > 0".to_string());
            }
            if ws.enabled.unwrap_or(false) {
                let has_nested = ws.forward.is_some() || ws.reverse.is_some();
                if !has_nested && ws.url.is_none() && ws.port.is_none() {
                    warnings.push(
                        "connect.websocket enabled but neither url nor port is set".to_string(),
                    );
                }
            }
            if let Some(forward) = &ws.forward
                && forward.enabled.unwrap_or(false)
                && forward.url.is_none()
                && (forward.port.is_none() || forward.host.as_deref().is_none())
            {
                warnings.push(
                    "connect.websocket.forward enabled but url is missing and host/port is incomplete"
                        .to_string(),
                );
            }
            if let Some(forward) = &ws.forward
                && forward.max_payload_size.is_some_and(|value| value == 0)
            {
                warnings
                    .push("connect.websocket.forward.max_payload_size should be > 0".to_string());
            }
            if let Some(forward) = &ws.forward
                && forward.max_connections.is_some_and(|value| value == 0)
            {
                warnings
                    .push("connect.websocket.forward.max_connections should be > 0".to_string());
            }
            if let Some(reverse) = &ws.reverse
                && reverse.enabled.unwrap_or(false)
                && reverse.url.is_none()
                && reverse.port.is_none()
            {
                warnings
                    .push("connect.websocket.reverse enabled but url/port is missing".to_string());
            }
            if let Some(reverse) = &ws.reverse
                && reverse.max_payload_size.is_some_and(|value| value == 0)
            {
                warnings
                    .push("connect.websocket.reverse.max_payload_size should be > 0".to_string());
            }
            if let Some(reverse) = &ws.reverse
                && reverse.max_connections.is_some_and(|value| value == 0)
            {
                warnings
                    .push("connect.websocket.reverse.max_connections should be > 0".to_string());
            }
        }

        if let Some(http) = &connect.tcp_http {
            if http.max_payload_size.is_some_and(|value| value == 0) {
                warnings.push("connect.tcp-http.max_payload_size should be > 0".to_string());
            }
            if http.max_connections.is_some_and(|value| value == 0) {
                warnings.push("connect.tcp-http.max_connections should be > 0".to_string());
            }
        }

        if let Some(sse) = &connect.sse {
            if sse.max_payload_size.is_some_and(|value| value == 0) {
                warnings.push("connect.sse.max_payload_size should be > 0".to_string());
            }
            if sse.max_connections.is_some_and(|value| value == 0) {
                warnings.push("connect.sse.max_connections should be > 0".to_string());
            }
        }
    }

    if let Some(onebot) = config_onebot_v11(doc) {
        for entry in &onebot.whitelist {
            if entry.as_token().is_empty() {
                warnings.push("onebot-v11.whitelist should not contain empty values".to_string());
                break;
            }
        }
    }

    warnings
}

pub(crate) fn prime_reload_warning_state(doc: &AppConfigDoc) {
    let mut lock = LAST_RELOAD_WARNING_STATE
        .lock()
        .expect("reload warning state lock should not be poisoned");
    *lock = Some(ReloadWarningState::from_doc(doc));
}

pub(crate) fn collect_runtime_reload_warnings(doc: &AppConfigDoc) -> Vec<String> {
    let current = ReloadWarningState::from_doc(doc);
    let mut lock = LAST_RELOAD_WARNING_STATE
        .lock()
        .expect("reload warning state lock should not be poisoned");
    let warnings = runtime_reload_warnings(lock.as_ref(), &current);
    *lock = Some(current);
    warnings
}

pub(crate) fn runtime_reload_warnings(
    previous: Option<&ReloadWarningState>,
    current: &ReloadWarningState,
) -> Vec<String> {
    let mut warnings = Vec::new();

    let runtime_changed = previous.is_none_or(|prev| prev.runtime != current.runtime);
    let runtime_sensitive = previous
        .is_some_and(|prev| runtime_has_hot_reload_sensitive_fields(&prev.runtime))
        || runtime_has_hot_reload_sensitive_fields(&current.runtime);
    if runtime_changed && runtime_sensitive {
        warnings.push(
            "runtime.worker_count/ingress_queue/worker_queue are low-level parameters; /reload will not hot-apply them. Restart is recommended, hot switching may cause unpredictable behavior.".to_string(),
        );
    }

    let log_changed = previous.is_none_or(|prev| prev.log != current.log);
    let log_sensitive = previous.is_some_and(|prev| log_has_startup_only_fields(&prev.log))
        || log_has_startup_only_fields(&current.log);
    if log_changed && log_sensitive {
        warnings.push(
            "log mode/level/timestamp parameters are loaded at startup and may not be fully applied by /reload. Restart is recommended for deterministic behavior.".to_string(),
        );
    }

    warnings
}

pub(crate) fn runtime_has_hot_reload_sensitive_fields(
    runtime: &Option<RuntimeConfigSection>,
) -> bool {
    runtime.as_ref().is_some_and(|runtime| {
        runtime.worker_count.is_some()
            || runtime.ingress_queue.is_some()
            || runtime.worker_queue.is_some()
    })
}

pub(crate) fn log_has_startup_only_fields(log: &Option<LogConfigSection>) -> bool {
    log.as_ref().is_some_and(|log| {
        log.mode.is_some()
            || log.level.is_some()
            || log.timezone.is_some()
            || log.timestamp_format.is_some()
            || log.timestamp_pattern.is_some()
    })
}
