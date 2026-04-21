use std::collections::HashMap;
use std::fs;
use std::io;
use std::net::{
    IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener, TcpStream as StdTcpStream,
};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::app_host::AppHostSnapshot;
use crate::observability::{BufferedLogEntry, recent_buffered_logs};
use crate::{LogLevel, emit_console_log};

const DEFAULT_HTTP_PORT: u16 = 14500;
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const REQUEST_READ_CHUNK_BYTES: usize = 2048;
const HEALTH_ROUTE: &str = "/api/health";
const LOGS_ROUTE: &str = "/api/logs";
const LOGS_ROUTE_LIMIT: usize = 200;
const DEV_FRONTEND_PROBE_TIMEOUT: Duration = Duration::from_millis(150);

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
            let (socket, _) = listener.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(err) = server.handle_connection(socket).await {
                    emit_console_log(
                        LogLevel::Warn,
                        "web.host",
                        format!("failed to serve shared HTTP request: {err}"),
                    );
                }
            });
        }
    }

    async fn handle_connection(&self, mut socket: TcpStream) -> io::Result<()> {
        let request = read_http_request(&mut socket).await?;
        if request.is_empty() {
            return Ok(());
        }

        let response = self.route_http_request(&request);
        socket.write_all(&response).await?;
        socket.shutdown().await
    }

    fn route_http_request(&self, request: &[u8]) -> Vec<u8> {
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

        if let Some(asset) = self.assets.static_asset_for_path(path) {
            return build_response("200 OK", asset.content_type(), asset.body(), is_head);
        }

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

    fn dev_frontend_redirect(
        &self,
        raw_path: &str,
        path: &str,
        request: &str,
    ) -> Option<String> {
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

async fn read_http_request(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0_u8; REQUEST_READ_CHUNK_BYTES];
    let mut request = Vec::new();

    loop {
        let read = socket.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|slice| slice == b"\r\n\r\n")
            || request.len() >= MAX_REQUEST_BYTES
        {
            break;
        }
    }

    Ok(request)
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let path = parts.next()?;
    Some((method, path))
}

fn build_response(status: &str, content_type: &str, body: &[u8], head_only: bool) -> Vec<u8> {
    let response_body = if head_only { &[][..] } else { body };
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut response = headers.into_bytes();
    response.extend_from_slice(response_body);
    response
}

fn build_redirect_response(status: &str, location: &str, head_only: bool) -> Vec<u8> {
    let response_body = if head_only {
        &[][..]
    } else {
        b"redirecting"
    };
    let headers = format!(
        "HTTP/1.1 {status}\r\nLocation: {location}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        response_body.len()
    );
    let mut response = headers.into_bytes();
    response.extend_from_slice(response_body);
    response
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

fn parse_named_header<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (header, value) = line.split_once(':')?;
    if header.trim().eq_ignore_ascii_case(name) {
        Some(value.trim())
    } else {
        None
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

    fn test_server() -> WebHostService {
        WebHostService {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_HTTP_PORT),
            browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            dev_frontend: None,
            snapshot_provider: Arc::new(|| AppHostSnapshot {
                status: "running".to_string(),
                runtime_target: "tauri2".to_string(),
                adapter_count: 3,
                ..AppHostSnapshot::default()
            }),
            assets: Arc::new(test_assets()),
        }
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

    #[test]
    fn root_route_returns_injected_html() {
        let response =
            test_server().route_http_request(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let (headers, body) = split_response(response);
        let body = String::from_utf8(body).expect("body should be utf8");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert_eq!(body, TEST_HTML);
    }

    #[test]
    fn health_route_returns_runtime_metadata() {
        let response = test_server()
            .route_http_request(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let (headers, body) = split_response(response);
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("health body should be valid json");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(body["runtime"]["runtime_target"], "tauri2");
        assert_eq!(body["runtime"]["status"], "running");
        assert_eq!(body["runtime"]["adapter_count"], 3);
        assert_eq!(body["runtime"]["resource_usage"]["cpu"]["system_percent"], 0.0);
        assert_eq!(body["bind"], "0.0.0.0:14500");
        assert_eq!(body["desktop_url"], "http://127.0.0.1:14500/");
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

        let response =
            test_server().route_http_request(b"GET /api/logs HTTP/1.1\r\nHost: localhost\r\n\r\n");
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
    fn static_asset_route_returns_injected_asset() {
        let response = test_server()
            .route_http_request(b"GET /assets/bot.svg HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let (headers, body) = split_response(response);
        let body = String::from_utf8(body).expect("body should be utf8");

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("Content-Type: image/svg+xml; charset=utf-8\r\n"));
        assert_eq!(body, TEST_SVG);
    }

    #[test]
    fn head_request_returns_headers_without_body() {
        let response =
            test_server().route_http_request(b"HEAD / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let (headers, body) = split_response(response);

        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains(&format!("Content-Length: {}\r\n", TEST_HTML.len())));
        assert!(body.is_empty());
    }

    #[test]
    fn missing_asset_returns_not_found() {
        let response =
            test_server().route_http_request(b"GET /missing HTTP/1.1\r\nHost: localhost\r\n\r\n");
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
        };

        let js_response =
            server.route_http_request(b"GET /assets/app.js HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let (js_headers, js_body) = split_response(js_response);
        let js_body = String::from_utf8(js_body).expect("js body should be utf8");
        assert!(js_headers.contains("Content-Type: text/javascript; charset=utf-8\r\n"));
        assert_eq!(js_body, "console.log('ok');");

        let spa_response =
            server.route_http_request(b"GET /dashboard HTTP/1.1\r\nHost: localhost\r\n\r\n");
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
        let probe_listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("probe listener should bind");
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
        };

        let response = server.route_http_request(
            b"GET /dashboard?tab=runtime HTTP/1.1\r\nHost: 192.168.2.2:14500\r\n\r\n",
        );
        let (headers, body) = split_response(response);

        assert!(headers.starts_with("HTTP/1.1 307 Temporary Redirect\r\n"));
        assert!(headers.contains("Location: http://192.168.2.2:1420/dashboard?tab=runtime\r\n"));
        assert_eq!(String::from_utf8(body).expect("body should be utf8"), "redirecting");
    }

    #[test]
    fn dev_frontend_redirect_keeps_local_api_and_static_routes() {
        let probe_listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("probe listener should bind");
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
        };

        let api_response =
            server.route_http_request(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let (api_headers, _) = split_response(api_response);
        assert!(api_headers.starts_with("HTTP/1.1 200 OK\r\n"));

        let icon_response =
            server.route_http_request(b"GET /favicon.ico HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let (icon_headers, _) = split_response(icon_response);
        assert!(icon_headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(icon_headers.contains("Content-Type: image/x-icon\r\n"));
    }
}
