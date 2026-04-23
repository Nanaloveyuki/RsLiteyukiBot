use std::sync::Arc;

use liteyukibot_core::RuntimeTarget;
use liteyukibot_core::app_host::EmbeddedAppHost;
use liteyukibot_core::web_host::{WebHostService, WebHostSnapshotProvider};
use liteyukibot_core::web_ui::{build_default_web_host_assets, build_default_web_host_config};
use liteyukibot_core::{LogLevel, emit_console_log};

const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Web;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_host = EmbeddedAppHost::start_for_target(resolve_runtime_target())
        .await
        .map_err(std::io::Error::other)?;
    let snapshot_provider: WebHostSnapshotProvider = {
        let app_host = app_host.clone();
        Arc::new(move || app_host.snapshot())
    };
    let assets = build_default_web_host_assets();
    let (server, listener) =
        WebHostService::bind(build_default_web_host_config(), snapshot_provider, assets)
            .map(|(server, listener)| (server.with_runtime_host(app_host.clone()), listener))
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
    emit_console_log(
        LogLevel::Info,
        "web.host",
        "Press Ctrl+C to stop the shared web host.",
    );

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
