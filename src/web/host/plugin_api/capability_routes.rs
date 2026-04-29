use super::*;

use super::capability_state::{
    all_plugin_capabilities_payload, plugin_capabilities_payload, plugin_diagnostics_payload,
    plugin_runtime_state_payload,
};

pub(super) fn route_capability_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    raw_path: &str,
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/Plugin/Capabilities" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "Plugin/Capabilities only accepts GET");
            return Some(napcat_response(body, is_head));
        }
        let query = parse_query_string(raw_path);
        let plugin_id = query
            .get("id")
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let body = match plugin_capabilities_payload(service, plugin_id) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Capabilities/All" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "Plugin/Capabilities/All only accepts GET");
            return Some(napcat_response(body, is_head));
        }
        let body = match all_plugin_capabilities_payload(service) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Tools" {
        return Some(route_capability_item(
            service,
            method,
            raw_path,
            is_head,
            "Plugin/Tools only accepts GET",
            |payload| {
                serde_json::json!({
                    "pluginId": payload.plugin_id,
                    "runtimeKind": payload.runtime_kind,
                    "support": payload.support.tools,
                    "items": payload.snapshot.tools,
                    "updatedAt": payload.snapshot.updated_at,
                })
            },
        ));
    }

    if api_path == "/Plugin/WebApis" {
        return Some(route_capability_item(
            service,
            method,
            raw_path,
            is_head,
            "Plugin/WebApis only accepts GET",
            |payload| {
                serde_json::json!({
                    "pluginId": payload.plugin_id,
                    "runtimeKind": payload.runtime_kind,
                    "support": payload.support.web_apis,
                    "items": payload.snapshot.web_apis,
                    "updatedAt": payload.snapshot.updated_at,
                })
            },
        ));
    }

    if api_path == "/Plugin/CronJobs" {
        return Some(route_capability_item(
            service,
            method,
            raw_path,
            is_head,
            "Plugin/CronJobs only accepts GET",
            |payload| {
                serde_json::json!({
                    "pluginId": payload.plugin_id,
                    "runtimeKind": payload.runtime_kind,
                    "support": payload.support.cron_jobs,
                    "items": payload.snapshot.cron_jobs,
                    "updatedAt": payload.snapshot.updated_at,
                })
            },
        ));
    }

    if api_path == "/Plugin/Tasks" {
        return Some(route_capability_item(
            service,
            method,
            raw_path,
            is_head,
            "Plugin/Tasks only accepts GET",
            |payload| {
                serde_json::json!({
                    "pluginId": payload.plugin_id,
                    "runtimeKind": payload.runtime_kind,
                    "support": payload.support.tasks,
                    "items": payload.snapshot.tasks,
                    "updatedAt": payload.snapshot.updated_at,
                })
            },
        ));
    }

    if api_path == "/Plugin/RuntimeState" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "Plugin/RuntimeState only accepts GET");
            return Some(napcat_response(body, is_head));
        }
        let query = parse_query_string(raw_path);
        let plugin_id = query
            .get("id")
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let body = match plugin_runtime_state_payload(service, plugin_id) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Diagnostics" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "Plugin/Diagnostics only accepts GET");
            return Some(napcat_response(body, is_head));
        }
        let query = parse_query_string(raw_path);
        let plugin_id = query
            .get("id")
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let body = match plugin_diagnostics_payload(service, plugin_id) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    None
}

fn route_capability_item(
    service: &WebHostService,
    method: &str,
    raw_path: &str,
    is_head: bool,
    method_error: &str,
    render: impl Fn(super::capability_state::PluginCapabilitiesPayload) -> Value,
) -> Vec<u8> {
    if !method.eq_ignore_ascii_case("GET") {
        let body = napcat_err(-1, method_error);
        return napcat_response(body, is_head);
    }
    let query = parse_query_string(raw_path);
    let plugin_id = query
        .get("id")
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    let body = match plugin_capabilities_payload(service, plugin_id) {
        Ok(payload) => napcat_ok(&render(payload)),
        Err(err) => napcat_err(-1, err.as_str()),
    };
    napcat_response(body, is_head)
}
