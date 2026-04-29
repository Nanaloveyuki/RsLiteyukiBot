#[path = "action_routes.rs"]
mod action_routes;
#[path = "capability_routes.rs"]
mod capability_routes;
#[path = "config_routes.rs"]
mod config_routes;
#[path = "store_routes.rs"]
mod store_routes;

use super::*;

pub(super) fn route_plugin_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    capability_routes::route_capability_api(service, method, api_path, raw_path, is_head)
        .or_else(|| {
            action_routes::route_action_api(service, method, api_path, raw_path, request, is_head)
        })
        .or_else(|| store_routes::route_store_api(service, api_path, raw_path, is_head))
        .or_else(|| {
            config_routes::route_config_api(service, method, api_path, raw_path, request, is_head)
        })
}

pub(super) fn plugin_runtime_tool_name(plugin_id: &str, tool_name: &str) -> String {
    format!("plugin::{}::{}", plugin_id.trim(), tool_name.trim())
}
