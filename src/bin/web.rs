use std::sync::Arc;
use std::{fs, path::Path};

use liteyukibot_core::RuntimeTarget;
use liteyukibot_core::app_host::EmbeddedAppHost;
use liteyukibot_core::web_host::{
    WebHostAsset, WebHostAssets, WebHostConfig, WebHostService, WebHostSnapshotProvider,
};
use liteyukibot_core::{LogLevel, emit_console_log};

const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Web;
const FRONTEND_DIST_DIR: &str = "frontend/dist";
const PLACEHOLDER_HTML: &str = include_str!("../../src-tauri/static/index.html");
const FRONTEND_LOGO_SVG: &str = include_str!("../../src-tauri/icons/bot.svg");
const WINDOW_ICON_ICO: &[u8] = include_bytes!("../../src-tauri/icons/bot.ico");

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_host = EmbeddedAppHost::start_for_target(resolve_runtime_target())
        .await
        .map_err(std::io::Error::other)?;
    let snapshot_provider: WebHostSnapshotProvider = {
        let app_host = app_host.clone();
        Arc::new(move || app_host.snapshot())
    };
    let assets = build_web_host_assets();
    let (server, listener) =
        WebHostService::bind(WebHostConfig::default(), snapshot_provider, assets)
            .map_err(std::io::Error::other)?;

    emit_console_log(
        LogLevel::Info,
        "web.host",
        format!(
            "shared web host listening on {} (desktop: {}, external: {})",
            server.bind_addr(),
            server.desktop_url(),
            server.external_url_hint(),
        ),
    );
    emit_console_log(LogLevel::Info, "web.host", "Press Ctrl+C to stop the shared web host.");

    let serve_task = tokio::spawn({
        let server = server.clone();
        async move { server.serve(listener).await }
    });

    tokio::signal::ctrl_c().await?;
    serve_task.abort();
    let _ = serve_task.await;
    app_host.shutdown().await.map_err(std::io::Error::other)?;
    Ok(())
}

fn resolve_runtime_target() -> RuntimeTarget {
    std::env::var("LY_RUNTIME_TARGET")
        .ok()
        .as_deref()
        .and_then(RuntimeTarget::parse)
        .unwrap_or(DEFAULT_RUNTIME_TARGET)
}

fn build_web_host_assets() -> WebHostAssets {
    let dist_dir = Path::new(FRONTEND_DIST_DIR);
    let index_asset = fs::read_to_string(dist_dir.join("index.html"))
        .map(|html| WebHostAsset::text("text/html; charset=utf-8", html))
        .unwrap_or_else(|_| WebHostAsset::text("text/html; charset=utf-8", PLACEHOLDER_HTML));

    let assets = WebHostAssets::new(index_asset)
        .with_asset(
            "/assets/bot.svg",
            WebHostAsset::text("image/svg+xml; charset=utf-8", FRONTEND_LOGO_SVG),
        )
        .with_asset(
            "/favicon.ico",
            WebHostAsset::binary("image/x-icon", WINDOW_ICON_ICO),
        );

    if dist_dir.exists() {
        assets.with_asset_directory(dist_dir.to_path_buf())
    } else {
        assets
    }
}
