#[path = "plugin_api/capability_state.rs"]
mod capability_state;
#[path = "plugin_api/route_handlers.rs"]
mod route_handlers;
#[path = "plugin_api/runtime_dispatch.rs"]
mod runtime_dispatch;

use super::*;

pub(super) fn route_plugin_runtime_web_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
    peer_ip: IpAddr,
) -> Option<Vec<u8>> {
    runtime_dispatch::route_plugin_runtime_web_api(
        service, method, api_path, raw_path, request, is_head, peer_ip,
    )
}

pub(super) fn route_plugin_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    route_handlers::route_plugin_api(service, method, api_path, raw_path, request, is_head)
}
