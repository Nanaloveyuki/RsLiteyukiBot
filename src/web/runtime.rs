use std::sync::Arc;

use tokio::net::TcpListener;

use crate::RuntimeTarget;
use crate::app_host::EmbeddedAppHost;
use crate::web::host::{WebHostService, WebHostSnapshotProvider};
use crate::web::ui::{build_default_web_host_assets, build_default_web_host_config};

pub struct EmbeddedWebRuntime {
    pub app_host: EmbeddedAppHost,
    pub server: WebHostService,
    pub listener: TcpListener,
}

impl EmbeddedWebRuntime {
    pub async fn start(target: RuntimeTarget) -> Result<Self, String> {
        let app_host = EmbeddedAppHost::start_for_target(target).await?;
        let snapshot_provider: WebHostSnapshotProvider = {
            let app_host = app_host.clone();
            Arc::new(move || app_host.snapshot())
        };
        let assets = build_default_web_host_assets();
        let (server, listener) =
            WebHostService::bind(build_default_web_host_config(), snapshot_provider, assets)
                .map(|(server, listener)| (server.with_runtime_host(app_host.clone()), listener))?;

        Ok(Self {
            app_host,
            server,
            listener,
        })
    }

    pub fn into_parts(self) -> (EmbeddedAppHost, WebHostService, TcpListener) {
        (self.app_host, self.server, self.listener)
    }
}
