use super::*;

pub(super) fn route_config_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
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
