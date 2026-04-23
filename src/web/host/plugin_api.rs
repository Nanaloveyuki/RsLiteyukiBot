use super::*;

pub(super) fn route_plugin_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/Plugin/List" {
        let payload = if let Some(runtime_host) = &service.runtime_host {
            build_runtime_plugin_payload(run_async_for_web_host(
                runtime_host.plugin_catalog_snapshot(),
            ))
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
        let body = match install_local_plugin_archive(request, service.runtime_host.as_ref()) {
            Ok(data) => napcat_ok(&data),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Store/List" {
        let query = parse_query_string(raw_path);
        let force_refresh = query
            .get("forceRefresh")
            .map(String::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("true"));
        let _ = force_refresh;
        let body = napcat_ok(&build_local_plugin_store_catalog(
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
        let catalog = build_local_plugin_store_catalog(service.runtime_host.as_ref());
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

    if api_path == "/Plugin/Config" {
        if method.eq_ignore_ascii_case("GET") {
            let query = parse_query_string(raw_path);
            let plugin_id = query
                .get("id")
                .map(String::as_str)
                .unwrap_or_default()
                .trim();
            if plugin_id.is_empty() {
                let body = napcat_err(-1, "missing plugin id");
                return Some(napcat_response(body, is_head));
            }
            let Some(descriptor) =
                resolve_plugin_descriptor(service.runtime_host.as_ref(), plugin_id)
            else {
                let body = napcat_err(-1, "plugin not found");
                return Some(napcat_response(body, is_head));
            };
            if !plugin_can_read_config(&descriptor)
                || plugin_declared_config_path(&descriptor).is_none()
            {
                let body = napcat_err(-1, "plugin does not expose readable WebUI config");
                return Some(napcat_response(body, is_head));
            }
            let config = match PluginSdk::default().read_explicit_config_document(&descriptor) {
                Ok(Value::Object(config)) => config,
                Ok(_) => {
                    let body = napcat_err(-1, "plugin config root must be an object");
                    return Some(napcat_response(body, is_head));
                }
                Err(err) => {
                    let message = err.to_string();
                    let body = napcat_err(-1, message.as_str());
                    return Some(napcat_response(body, is_head));
                }
            };
            let body = napcat_ok(&serde_json::json!({
                "schema": infer_plugin_config_schema(&config),
                "config": Value::Object(config),
                "supportReactive": false
            }));
            return Some(napcat_response(body, is_head));
        }
        let body = parse_json_body(request);
        let plugin_id = body
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if plugin_id.is_empty() {
            let body = napcat_err(-1, "missing plugin id");
            return Some(napcat_response(body, is_head));
        }
        let Some(descriptor) = resolve_plugin_descriptor(service.runtime_host.as_ref(), plugin_id)
        else {
            let body = napcat_err(-1, "plugin not found");
            return Some(napcat_response(body, is_head));
        };
        if !plugin_can_write_config(&descriptor)
            || plugin_declared_config_path(&descriptor).is_none()
        {
            let body = napcat_err(-1, "plugin does not expose writable WebUI config");
            return Some(napcat_response(body, is_head));
        }
        let Some(config) = body.get("config") else {
            let body = napcat_err(-1, "missing plugin config");
            return Some(napcat_response(body, is_head));
        };
        if !config.is_object() {
            let body = napcat_err(-1, "plugin config must be a JSON object");
            return Some(napcat_response(body, is_head));
        }
        if let Err(err) = PluginSdk::default().write_explicit_config_document(&descriptor, config) {
            let message = err.to_string();
            let body = napcat_err(-1, message.as_str());
            return Some(napcat_response(body, is_head));
        }
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Config/Change" {
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Config/SSE" {
        let event_data = serde_json::json!({ "type": "complete" }).to_string();
        return Some(sse_response(&event_data, is_head));
    }

    None
}
