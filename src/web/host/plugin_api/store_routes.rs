use super::*;

pub(super) fn route_store_api(
    service: &WebHostService,
    api_path: &str,
    raw_path: &str,
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/Plugin/Store/List" {
        let query = parse_query_string(raw_path);
        let force_refresh = query
            .get("forceRefresh")
            .map(String::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("true"));
        let _ = force_refresh;
        let body = napcat_ok(&super::plugin_store::build_local_plugin_store_catalog(
            service.runtime_host.as_ref(),
        ));
        return Some(napcat_response(body, is_head));
    }

    if api_path.starts_with("/Plugin/Store/Detail/") {
        let plugin_id = api_path.trim_start_matches("/Plugin/Store/Detail/").trim();
        if plugin_id.is_empty() {
            let body = napcat_err(-1, "missing plugin id");
            return Some(napcat_response(body, is_head));
        }
        let catalog =
            super::plugin_store::build_local_plugin_store_catalog(service.runtime_host.as_ref());
        let body = if let Some(plugin) = catalog
            .plugins
            .into_iter()
            .find(|item| item.id == plugin_id)
        {
            napcat_ok(&plugin)
        } else {
            napcat_err(-1, "Plugin not found")
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Store/Install" {
        let body = napcat_err(
            -1,
            "Plugin store install is not supported by the current runtime",
        );
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Store/Install/SSE" {
        let event_data = serde_json::json!({
            "error": "Plugin store install is not supported by the current runtime"
        })
        .to_string();
        return Some(sse_response(&event_data, is_head));
    }

    None
}
