use std::collections::HashSet;
use std::path::{Path, PathBuf};

use liteyukibot_core::{AdapterConfig, LiteyukiBot, LogLevel, RuntimeSettings, RuntimeTarget};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;

mod tui;

const APP_TITLE: &str = "RsLiteyukiBot";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;
const APP_CONFIG_PATHS: [&str; 6] = [
    "config.yaml",
    "rust-config.yaml",
    "rust-config.yml",
    "rust-config.toml",
    "config/rust-core.yaml",
    "config/rust-core.toml",
];

#[derive(Debug, Deserialize, Default)]
struct AppConfigDoc {
    #[serde(default)]
    rust: Option<AppRustSection>,
    #[serde(default)]
    runtime: Option<RuntimeConfigSection>,
    #[serde(default)]
    log: Option<LogConfigSection>,
    #[serde(default)]
    adapters: Option<Vec<AdapterConfig>>,
    #[serde(default)]
    connect: Option<ConnectConfigSection>,
    #[serde(default)]
    tui: Option<TuiConfigSection>,
}

#[derive(Debug, Deserialize, Default)]
struct AppRustSection {
    #[serde(default)]
    runtime: Option<RuntimeConfigSection>,
    #[serde(default)]
    log: Option<LogConfigSection>,
    #[serde(default)]
    adapters: Option<Vec<AdapterConfig>>,
    #[serde(default)]
    tui: Option<TuiConfigSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RuntimeConfigSection {
    #[serde(default)]
    worker_count: Option<usize>,
    #[serde(default)]
    ingress_queue: Option<usize>,
    #[serde(default)]
    worker_queue: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LogConfigSection {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    timestamp_format: Option<String>,
    #[serde(default)]
    timestamp_pattern: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ConnectConfigSection {
    #[serde(default)]
    websocket: Option<WebSocketConnectSection>,
    #[serde(default, rename = "tcp-http")]
    tcp_http: Option<HttpConnectSection>,
    #[serde(default)]
    sse: Option<SseConnectSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct WebSocketConnectSection {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    queue_capacity: Option<usize>,
    #[serde(default)]
    inbound_topic: Option<String>,
    #[serde(default)]
    outbound_topic: Option<String>,
    #[serde(default)]
    forward: Option<WebSocketEndpointSection>,
    #[serde(default)]
    reverse: Option<WebSocketEndpointSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct WebSocketEndpointSection {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    queue_capacity: Option<usize>,
    #[serde(default)]
    inbound_topic: Option<String>,
    #[serde(default)]
    outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct HttpConnectSection {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    queue_capacity: Option<usize>,
    #[serde(default)]
    inbound_topic: Option<String>,
    #[serde(default)]
    outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SseConnectSection {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    queue_capacity: Option<usize>,
    #[serde(default)]
    inbound_topic: Option<String>,
    #[serde(default)]
    outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct TuiConfigSection {
    #[serde(default)]
    resume: Option<TuiResumeSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct TuiResumeSection {
    #[serde(default)]
    store_path: Option<String>,
    #[serde(default)]
    max_sessions: Option<usize>,
    #[serde(default)]
    max_size_mib: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AdapterConfigDoc {
    adapters: Vec<AdapterConfig>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(err) = ensure_default_config_files() {
        eprintln!("failed to ensure default config files: {err}");
    }

    let settings = match RuntimeSettings::try_load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("failed to load runtime config from file/env, fallback to default: {err}");
            RuntimeSettings::default()
        }
    };
    let _ = settings.clone().install_global();
    let active_settings = RuntimeSettings::global_or_default();
    let mut runtime_config = RuntimeSettings::global_runtime_config().clone();
    runtime_config.logger.min_level = LogLevel::Error;

    let app_config = load_app_config();
    let target = resolve_runtime_target();
    let adapter_configs = load_adapter_configs(&app_config).unwrap_or_default();
    let adapter_autostart = !adapter_configs.is_empty();
    let tui_config = resolve_tui_config(&app_config);

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<tui::UiEvent>();
    let ui_tx_for_handler = ui_tx.clone();

    let mut bot = LiteyukiBot::builder(APP_TITLE, env!("CARGO_PKG_VERSION"))
        .with_target(target)
        .with_runtime_config(runtime_config)
        .with_adapter_configs(adapter_configs.clone())
        .with_adapter_autostart(adapter_autostart)
        .with_event_handler(move |event, _logger| {
            let ui_tx_for_handler = ui_tx_for_handler.clone();
            async move {
                let _ = ui_tx_for_handler.send(tui::UiEvent::RuntimeHandled {
                    id: event.id,
                    topic: event.topic.clone(),
                    payload_preview: payload_preview(&event.payload),
                });
            }
        })
        .build();

    let tx_before = ui_tx.clone();
    bot.on_before_start_sync("tui-before-start", Default::default(), move |_context| {
        let _ = tx_before.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: "runtime preparing...".to_string(),
        });
        Ok(())
    });

    let tx_after = ui_tx.clone();
    bot.on_after_start_sync("tui-after-start", Default::default(), move |_context| {
        let _ = tx_after.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: "runtime started".to_string(),
        });
        Ok(())
    });

    let tx_before_shutdown = ui_tx.clone();
    bot.on_before_process_shutdown_sync(
        "tui-before-shutdown",
        Default::default(),
        move |_context, process_name| {
            let _ = tx_before_shutdown.send(tui::UiEvent::Log {
                level: tui::UiLevel::Warn,
                message: format!("shutting down process: {}", process_name),
            });
            Ok(())
        },
    );

    bot.start().await?;

    let tui_result = tui::run(
        &mut bot,
        target,
        active_settings.describe(),
        adapter_configs,
        adapter_autostart,
        tui_config,
        reload_from_config,
        &mut ui_rx,
    )
    .await;

    let shutdown_result = bot.shutdown().await;
    if let Err(err) = shutdown_result {
        eprintln!("bot shutdown failed: {err}");
    }

    tui_result?;
    Ok(())
}

fn resolve_runtime_target() -> RuntimeTarget {
    std::env::var("LY_RUNTIME_TARGET")
        .ok()
        .as_deref()
        .and_then(RuntimeTarget::parse)
        .unwrap_or(DEFAULT_RUNTIME_TARGET)
}

fn load_app_config() -> AppConfigDoc {
    let Some(path) = resolve_app_config_path() else {
        return AppConfigDoc::default();
    };

    match load_app_config_from_path(&path) {
        Ok(doc) => {
            for warning in validate_app_config(&doc) {
                eprintln!("config warning ({}): {warning}", path.display());
            }
            doc
        }
        Err(err) => {
            eprintln!("failed to load app config from {}: {err}", path.display());
            AppConfigDoc::default()
        }
    }
}

fn resolve_app_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("LY_CONFIG_PATH") {
        return Some(PathBuf::from(path));
    }

    APP_CONFIG_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn ensure_default_config_files() -> Result<(), Box<dyn std::error::Error>> {
    write_default_config_if_missing(Path::new("config.yaml"))?;

    if let Ok(path) = std::env::var("LY_CONFIG_PATH") {
        let path = PathBuf::from(path);
        write_default_config_if_missing(path.as_path())?;
    }

    Ok(())
}

fn write_default_config_if_missing(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
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
    timeout_seconds: 30
  tcp-http:
    enabled: true
    host: 127.0.0.1
    port: 8081
    path: /
    timeout_seconds: 30
  sse:
    enabled: true
    host: 127.0.0.1
    port: 8082
    path: /sse
    timeout_seconds: 30
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
timeout_seconds = 30

[connect.tcp-http]
enabled = true
host = "127.0.0.1"
port = 8081
path = "/"
timeout_seconds = 30

[connect.sse]
enabled = true
host = "127.0.0.1"
port = 8082
path = "/sse"
timeout_seconds = 30
"#;

fn load_app_config_from_path(path: &Path) -> Result<AppConfigDoc, Box<dyn std::error::Error>> {
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

fn load_adapter_configs(
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

fn resolve_tui_config(app_config: &AppConfigDoc) -> tui::TuiConfig {
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

fn connect_to_adapter_configs(doc: &AppConfigDoc) -> Vec<AdapterConfig> {
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

    let Some(url) = url else {
        return None;
    };

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
    }
}

fn build_url(scheme: &str, host: &str, port: Option<u16>, path: &str) -> Option<String> {
    let port = port?;
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Some(format!("{scheme}://{host}:{port}{normalized_path}"))
}

fn seconds_to_timeout_ms(seconds: Option<u64>) -> u64 {
    seconds.unwrap_or(5).saturating_mul(1000).max(10)
}

fn payload_preview(payload: &Value) -> String {
    let raw = payload.to_string();
    const MAX: usize = 96;
    let mut iter = raw.chars();
    let preview: String = iter.by_ref().take(MAX).collect();
    if iter.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn reload_from_config(bot: &mut LiteyukiBot) -> tui::ReloadFuture<'_> {
    Box::pin(async move {
        let app_config = load_app_config();
        let warnings = runtime_reload_warnings(&app_config);
        let adapters = load_adapter_configs(&app_config)
            .map_err(|err| format!("failed to load adapter configs: {err}"))?;
        let autostart = !adapters.is_empty();
        bot.reload_adapters(adapters.clone(), autostart)
            .await
            .map_err(|err| format!("failed to apply adapter reload: {err}"))?;
        let tui_config = resolve_tui_config(&app_config);
        Ok(tui::ReloadResult {
            adapters,
            adapter_autostart: autostart,
            tui_config,
            warnings,
        })
    })
}

fn sanitize_adapter_configs(configs: Vec<AdapterConfig>, source: &str) -> Vec<AdapterConfig> {
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

fn validate_app_config(doc: &AppConfigDoc) -> Vec<String> {
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
            if let Some(reverse) = &ws.reverse
                && reverse.enabled.unwrap_or(false)
                && reverse.url.is_none()
                && reverse.port.is_none()
            {
                warnings
                    .push("connect.websocket.reverse enabled but url/port is missing".to_string());
            }
        }
    }

    warnings.extend(runtime_reload_warnings(doc));

    warnings
}

fn runtime_reload_warnings(doc: &AppConfigDoc) -> Vec<String> {
    let mut warnings = Vec::new();

    if let Some(runtime) = config_runtime(doc)
        && (runtime.worker_count.is_some()
            || runtime.ingress_queue.is_some()
            || runtime.worker_queue.is_some())
    {
        warnings.push(
            "runtime.worker_count/ingress_queue/worker_queue are low-level parameters; /reload will not hot-apply them. Restart is recommended, hot switching may cause unpredictable behavior.".to_string(),
        );
    }

    if let Some(log) = config_log(doc)
        && (log.mode.is_some()
            || log.level.is_some()
            || log.timezone.is_some()
            || log.timestamp_format.is_some()
            || log.timestamp_pattern.is_some())
    {
        warnings.push(
            "log mode/level/timestamp parameters are loaded at startup and may not be fully applied by /reload. Restart is recommended for deterministic behavior.".to_string(),
        );
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str, ext: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!("rsliteyuki-{name}-{nanos}.{ext}"));
        path
    }

    #[test]
    fn write_default_config_if_missing_creates_yaml_template() {
        let path = temp_path("config-create", "yaml");
        let _ = std::fs::remove_file(&path);

        write_default_config_if_missing(&path).expect("config file should be created");
        let content = std::fs::read_to_string(&path).expect("config file should be readable");
        assert!(content.contains("rust:"));
        assert!(content.contains("adapters: []"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validate_app_config_reports_invalid_values() {
        let mut duplicate = AdapterConfig::default();
        duplicate.id = "dup".to_string();

        let mut invalid = AdapterConfig::default();
        invalid.id = "dup".to_string();
        invalid.endpoint.url = "".to_string();

        let doc = AppConfigDoc {
            rust: Some(AppRustSection {
                runtime: None,
                log: None,
                adapters: Some(vec![duplicate, invalid]),
                tui: Some(TuiConfigSection {
                    resume: Some(TuiResumeSection {
                        store_path: Some("   ".to_string()),
                        max_sessions: Some(0),
                        max_size_mib: Some(0),
                    }),
                }),
            }),
            runtime: None,
            log: None,
            adapters: None,
            connect: None,
            tui: None,
        };

        let warnings = validate_app_config(&doc);
        assert!(warnings.iter().any(|w| w.contains("duplicated adapter id")));
        assert!(warnings.iter().any(|w| w.contains("invalid adapter")));
        assert!(warnings.iter().any(|w| w.contains("store_path")));
        assert!(warnings.iter().any(|w| w.contains("max_sessions")));
        assert!(warnings.iter().any(|w| w.contains("max_size_mib")));
    }

    #[test]
    fn runtime_reload_warnings_detect_low_level_runtime_fields() {
        let doc = AppConfigDoc {
            rust: Some(AppRustSection {
                runtime: Some(RuntimeConfigSection {
                    worker_count: Some(8),
                    ingress_queue: None,
                    worker_queue: None,
                }),
                log: None,
                adapters: None,
                tui: None,
            }),
            runtime: None,
            log: None,
            adapters: None,
            connect: None,
            tui: None,
        };

        let warnings = runtime_reload_warnings(&doc);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("hot switching may cause unpredictable behavior"))
        );
    }

    #[test]
    fn connect_websocket_both_mode_generates_forward_and_reverse_adapters() {
        let doc = AppConfigDoc {
            rust: None,
            runtime: None,
            log: None,
            adapters: None,
            connect: Some(ConnectConfigSection {
                websocket: Some(WebSocketConnectSection {
                    enabled: Some(true),
                    mode: Some("both".to_string()),
                    url: Some("ws://127.0.0.1:3000/ws".to_string()),
                    host: Some("0.0.0.0".to_string()),
                    port: Some(8080),
                    path: Some("/ws".to_string()),
                    headers: None,
                    token: None,
                    timeout_seconds: Some(30),
                    queue_capacity: Some(256),
                    inbound_topic: None,
                    outbound_topic: None,
                    forward: None,
                    reverse: None,
                }),
                tcp_http: None,
                sse: None,
            }),
            tui: None,
        };

        let adapters = load_adapter_configs(&doc).expect("connect adapters should parse");
        assert!(
            adapters
                .iter()
                .any(|adapter| adapter.id == "connect-ws-forward")
        );
        assert!(
            adapters
                .iter()
                .any(|adapter| adapter.id == "connect-ws-reverse")
        );
    }

    #[test]
    fn connect_websocket_port_without_mode_defaults_to_reverse() {
        let doc = AppConfigDoc {
            rust: None,
            runtime: None,
            log: None,
            adapters: None,
            connect: Some(ConnectConfigSection {
                websocket: Some(WebSocketConnectSection {
                    enabled: Some(true),
                    mode: None,
                    url: None,
                    host: Some("0.0.0.0".to_string()),
                    port: Some(8090),
                    path: Some("/ws".to_string()),
                    headers: None,
                    token: None,
                    timeout_seconds: Some(30),
                    queue_capacity: None,
                    inbound_topic: None,
                    outbound_topic: None,
                    forward: None,
                    reverse: None,
                }),
                tcp_http: None,
                sse: None,
            }),
            tui: None,
        };

        let adapters = load_adapter_configs(&doc).expect("connect adapters should parse");
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].id, "connect-ws-reverse");
    }
}
