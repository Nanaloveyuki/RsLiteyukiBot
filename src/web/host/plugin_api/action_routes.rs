use super::*;

pub(super) fn route_action_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    _raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/Plugin/Tools/Execute" {
        if !method.eq_ignore_ascii_case("POST") {
            let body = napcat_err(-1, "Plugin/Tools/Execute only accepts POST");
            return Some(napcat_response(body, is_head));
        }
        let body = parse_json_body(request);
        let plugin_id = body
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let tool_name = body
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if plugin_id.is_empty() || tool_name.is_empty() {
            let body = napcat_err(-1, "missing plugin id or tool name");
            return Some(napcat_response(body, is_head));
        }
        let arguments = body.get("arguments").cloned().unwrap_or(Value::Null);
        let runtime_host = match service.runtime_host.as_ref() {
            Some(runtime_host) => runtime_host,
            None => {
                let body = napcat_err(-1, "plugin runtime host is unavailable");
                return Some(napcat_response(body, is_head));
            }
        };
        let body = match run_async_for_web_host(
            runtime_host.execute_plugin_tool(plugin_id, tool_name, &arguments),
        ) {
            Ok(Some(output)) => napcat_ok(&serde_json::json!({
                "pluginId": plugin_id,
                "toolName": tool_name,
                "runtimeName": plugin_runtime_tool_name(plugin_id, tool_name),
                "output": output,
            })),
            Ok(None) => napcat_err(-1, "plugin tool not found"),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/List" {
        let payload = if let Some(runtime_host) = &service.runtime_host {
            build_runtime_plugin_payload(
                runtime_host,
                run_async_for_web_host(runtime_host.plugin_catalog_snapshot()),
            )
        } else {
            serde_json::json!({
                "plugins": discover_plugins(),
                "pluginManagerNotFound": false,
                "extensionPages": []
            })
        };
        let body = napcat_ok(&payload);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/RegisterManager" {
        let body = napcat_ok(&serde_json::json!({
            "message": format!("plugin manager ready ({} discovered)", discover_plugins().len())
        }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/SetStatus" {
        let body = parse_json_body(request);
        let (id, enable) = match (
            body.get("id").and_then(Value::as_str),
            body.get("enable").and_then(Value::as_bool),
        ) {
            (Some(id), Some(enable)) if !id.trim().is_empty() => (id.trim(), enable),
            _ => {
                let body = napcat_err(-1, "missing plugin id or enable flag");
                return Some(napcat_response(body, is_head));
            }
        };
        if let Some(runtime_host) = &service.runtime_host {
            let previous_disabled =
                run_async_for_web_host(runtime_host.plugin_catalog_snapshot()).disabled_plugin_ids;
            let mut next_disabled = previous_disabled.clone();
            if enable {
                next_disabled.retain(|entry| entry != id);
            } else if !next_disabled.iter().any(|entry| entry == id) {
                next_disabled.push(id.to_string());
            }
            let Some(config_path) = resolve_app_config_path() else {
                let body = napcat_err(-1, "app config path not found");
                return Some(napcat_response(body, is_head));
            };
            if let Err(err) = persist_disabled_plugins(config_path.as_path(), &next_disabled) {
                let body = napcat_err(-1, err.as_str());
                return Some(napcat_response(body, is_head));
            }
            if let Err(err) =
                run_async_for_web_host(runtime_host.apply_disabled_plugins(next_disabled))
            {
                let _ = persist_disabled_plugins(config_path.as_path(), &previous_disabled);
                let body = napcat_err(-1, err.as_str());
                return Some(napcat_response(body, is_head));
            }
        } else if let Err(err) = update_disabled_plugins(id, enable) {
            let body = napcat_err(-1, err.as_str());
            return Some(napcat_response(body, is_head));
        }
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Uninstall" {
        let body = napcat_err(
            -1,
            "Plugin uninstall is not supported by the current runtime",
        );
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Import" {
        let body = match super::plugin_install::install_local_plugin_archive(
            request,
            service.runtime_host.as_ref(),
        ) {
            Ok(data) => napcat_ok(&data),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    None
}
