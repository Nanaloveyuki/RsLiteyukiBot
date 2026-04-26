use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginCapabilitySupportState {
    registered: bool,
    executable: bool,
    persistent: bool,
    active: bool,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginCapabilitySupportSummary {
    tools: PluginCapabilitySupportState,
    web_apis: PluginCapabilitySupportState,
    cron_jobs: PluginCapabilitySupportState,
    tasks: PluginCapabilitySupportState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginCapabilitiesPayload {
    plugin_id: String,
    runtime_kind: crate::PluginRuntimeKind,
    support: PluginCapabilitySupportSummary,
    snapshot: crate::PluginCapabilitySnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginRuntimeBindingSummary {
    tools: bool,
    web_apis: bool,
    cron_jobs: bool,
    tasks: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginRuntimeStatePayload {
    plugin_id: String,
    runtime_kind: crate::PluginRuntimeKind,
    loaded: bool,
    enabled: bool,
    active: bool,
    snapshot_extracted: bool,
    executable_bindings: PluginRuntimeBindingSummary,
    scheduler_status: String,
    task_runtime_status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginDiagnosticsPayload {
    plugin_id: String,
    runtime_kind: crate::PluginRuntimeKind,
    load_state: String,
    snapshot_extracted: bool,
    executable_bindings: PluginRuntimeBindingSummary,
    scheduler_status: String,
    last_web_api_dispatch: crate::PluginExecutionRecord,
    last_tool_execution: crate::PluginExecutionRecord,
    last_cron_execution: crate::PluginExecutionRecord,
}

pub(super) fn route_plugin_runtime_web_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
    peer_ip: IpAddr,
) -> Option<Vec<u8>> {
    let prefix = "/Plugin/Runtime/WebApi/";
    let rest = api_path.strip_prefix(prefix)?;
    let (plugin_id, route_rest) = rest.split_once('/').unwrap_or((rest, ""));
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || route_rest.trim().is_empty() {
        return Some(build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin runtime web api not found",
            is_head,
        ));
    }

    let runtime_host = match service.runtime_host.as_ref() {
        Some(runtime_host) => runtime_host,
        None => {
            return Some(build_response(
                "503 Service Unavailable",
                "text/plain; charset=utf-8",
                b"plugin runtime host is unavailable",
                is_head,
            ));
        }
    };

    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let disabled = catalog
        .disabled_plugin_ids
        .iter()
        .any(|entry| entry == plugin_id);
    let Some(entry) = catalog
        .entries
        .into_iter()
        .find(|entry| entry.descriptor.metadata.id == plugin_id)
    else {
        return Some(build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin not found",
            is_head,
        ));
    };
    if !entry.loaded || disabled {
        return Some(build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin runtime web api not found",
            is_head,
        ));
    }

    let registered_route = normalize_registered_web_api_route(route_rest);
    let snapshot = match run_async_for_web_host(runtime_host.plugin_capability_snapshot(plugin_id))
    {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return Some(build_response(
                "500 Internal Server Error",
                "text/plain; charset=utf-8",
                format!("failed to read plugin capability snapshot: {err}").as_bytes(),
                is_head,
            ));
        }
    };
    let Some(snapshot) = snapshot else {
        return Some(build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin runtime web api not found",
            is_head,
        ));
    };
    let route_matches = snapshot
        .web_apis
        .iter()
        .filter(|web_api| web_api.route == registered_route)
        .collect::<Vec<_>>();
    if route_matches.is_empty() {
        return Some(build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin runtime web api not found",
            is_head,
        ));
    }
    let method_matches = route_matches
        .iter()
        .filter(|web_api| {
            web_api
                .methods
                .iter()
                .any(|registered| registered.eq_ignore_ascii_case(method))
        })
        .collect::<Vec<_>>();
    if method_matches.is_empty() {
        return Some(build_response(
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"method not allowed",
            is_head,
        ));
    }
    if method_matches.len() > 1 {
        return Some(build_response(
            "409 Conflict",
            "text/plain; charset=utf-8",
            b"conflicting plugin runtime web api registrations",
            is_head,
        ));
    }

    let dispatch_request = crate::PluginWebApiRequest {
        method: method.to_string(),
        path: registered_route.clone(),
        query: parse_query_string(raw_path),
        headers: parse_request_headers(request),
        body: extract_body(request).to_vec(),
        peer_ip: Some(peer_ip.to_string()),
    };
    let response = match run_async_for_web_host(runtime_host.dispatch_plugin_web_api(
        plugin_id,
        registered_route.as_str(),
        &dispatch_request,
    )) {
        Ok(Some(response)) => response,
        Ok(None) => {
            return Some(build_response(
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"plugin runtime web api not found",
                is_head,
            ));
        }
        Err(err) => {
            return Some(build_response(
                "500 Internal Server Error",
                "text/plain; charset=utf-8",
                format!("plugin runtime web api execution failed: {err}").as_bytes(),
                is_head,
            ));
        }
    };

    Some(build_response(
        http_status_line(response.status_code).as_str(),
        response.content_type.as_str(),
        response.body.as_slice(),
        is_head,
    ))
}

pub(super) fn route_plugin_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
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
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "Plugin/Tools only accepts GET");
            return Some(napcat_response(body, is_head));
        }
        let query = parse_query_string(raw_path);
        let plugin_id = query
            .get("id")
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let body = match plugin_capabilities_payload(service, plugin_id) {
            Ok(payload) => napcat_ok(&serde_json::json!({
                "pluginId": payload.plugin_id,
                "runtimeKind": payload.runtime_kind,
                "support": payload.support.tools,
                "items": payload.snapshot.tools,
                "updatedAt": payload.snapshot.updated_at,
            })),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/WebApis" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "Plugin/WebApis only accepts GET");
            return Some(napcat_response(body, is_head));
        }
        let query = parse_query_string(raw_path);
        let plugin_id = query
            .get("id")
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let body = match plugin_capabilities_payload(service, plugin_id) {
            Ok(payload) => napcat_ok(&serde_json::json!({
                "pluginId": payload.plugin_id,
                "runtimeKind": payload.runtime_kind,
                "support": payload.support.web_apis,
                "items": payload.snapshot.web_apis,
                "updatedAt": payload.snapshot.updated_at,
            })),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/CronJobs" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "Plugin/CronJobs only accepts GET");
            return Some(napcat_response(body, is_head));
        }
        let query = parse_query_string(raw_path);
        let plugin_id = query
            .get("id")
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let body = match plugin_capabilities_payload(service, plugin_id) {
            Ok(payload) => napcat_ok(&serde_json::json!({
                "pluginId": payload.plugin_id,
                "runtimeKind": payload.runtime_kind,
                "support": payload.support.cron_jobs,
                "items": payload.snapshot.cron_jobs,
                "updatedAt": payload.snapshot.updated_at,
            })),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Plugin/Tasks" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "Plugin/Tasks only accepts GET");
            return Some(napcat_response(body, is_head));
        }
        let query = parse_query_string(raw_path);
        let plugin_id = query
            .get("id")
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let body = match plugin_capabilities_payload(service, plugin_id) {
            Ok(payload) => napcat_ok(&serde_json::json!({
                "pluginId": payload.plugin_id,
                "runtimeKind": payload.runtime_kind,
                "support": payload.support.tasks,
                "items": payload.snapshot.tasks,
                "updatedAt": payload.snapshot.updated_at,
            })),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

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

fn plugin_capabilities_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginCapabilitiesPayload, String> {
    if plugin_id.trim().is_empty() {
        return Err("missing plugin id".to_string());
    }
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "plugin runtime host is unavailable".to_string())?;
    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let entry = catalog
        .entries
        .into_iter()
        .find(|entry| entry.descriptor.metadata.id == plugin_id)
        .ok_or_else(|| "plugin not found".to_string())?;
    let disabled = catalog
        .disabled_plugin_ids
        .iter()
        .any(|entry_id| entry_id == plugin_id);
    let runtime_kind = entry.descriptor.runtime.kind;
    let active = entry.loaded && !disabled;
    let snapshot = run_async_for_web_host(runtime_host.plugin_capability_snapshot(plugin_id))
        .map_err(|err| format!("failed to read plugin capability snapshot: {err}"))?
        .unwrap_or_else(|| empty_plugin_capability_snapshot(plugin_id, runtime_kind));
    let support = build_plugin_capability_support(service, &snapshot, plugin_id, active);
    Ok(PluginCapabilitiesPayload {
        plugin_id: plugin_id.to_string(),
        runtime_kind,
        support,
        snapshot,
    })
}

fn all_plugin_capabilities_payload(
    service: &WebHostService,
) -> Result<Vec<PluginCapabilitiesPayload>, String> {
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "plugin runtime host is unavailable".to_string())?;
    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let snapshots = run_async_for_web_host(runtime_host.all_plugin_capability_snapshots())
        .map_err(|err| format!("failed to read plugin capability snapshots: {err}"))?;
    let snapshot_map: std::collections::HashMap<String, crate::PluginCapabilitySnapshot> =
        std::collections::HashMap::from_iter(
            snapshots
                .into_iter()
                .map(|snapshot| (snapshot.plugin_id.clone(), snapshot)),
        );

    let disabled_ids = catalog.disabled_plugin_ids;
    let mut payloads = catalog
        .entries
        .into_iter()
        .map(|entry| {
            let plugin_id = entry.descriptor.metadata.id.clone();
            let runtime_kind = entry.descriptor.runtime.kind;
            let active = entry.loaded && !disabled_ids.iter().any(|id| id == &plugin_id);
            let snapshot = snapshot_map
                .get(plugin_id.as_str())
                .cloned()
                .unwrap_or_else(|| {
                    empty_plugin_capability_snapshot(plugin_id.as_str(), runtime_kind)
                });
            PluginCapabilitiesPayload {
                plugin_id: plugin_id.clone(),
                runtime_kind,
                support: build_plugin_capability_support(
                    service,
                    &snapshot,
                    plugin_id.as_str(),
                    active,
                ),
                snapshot,
            }
        })
        .collect::<Vec<_>>();
    payloads.sort_by(|left, right| left.plugin_id.cmp(&right.plugin_id));
    Ok(payloads)
}

fn empty_plugin_capability_snapshot(
    plugin_id: &str,
    runtime_kind: crate::PluginRuntimeKind,
) -> crate::PluginCapabilitySnapshot {
    crate::PluginCapabilitySnapshot {
        plugin_id: plugin_id.to_string(),
        runtime_kind,
        tools: Vec::new(),
        web_apis: Vec::new(),
        cron_jobs: Vec::new(),
        tasks: Vec::new(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn build_plugin_capability_support(
    service: &WebHostService,
    snapshot: &crate::PluginCapabilitySnapshot,
    plugin_id: &str,
    plugin_active: bool,
) -> PluginCapabilitySupportSummary {
    let cron_registered = !snapshot.cron_jobs.is_empty();
    let cron_enabled = snapshot.cron_jobs.iter().any(|job| job.enabled);
    let cron_executable = if plugin_active {
        service
            .runtime_host
            .as_ref()
            .and_then(|runtime_host| {
                run_async_for_web_host(runtime_host.plugin_has_executable_cron_jobs(plugin_id)).ok()
            })
            .unwrap_or_else(|| {
                snapshot
                    .cron_jobs
                    .iter()
                    .any(crate::llm::cron_task::cron_job_is_host_executable)
            })
    } else {
        false
    };
    PluginCapabilitySupportSummary {
        tools: build_tool_capability_support(snapshot, plugin_active),
        web_apis: build_web_api_capability_support(snapshot, plugin_active),
        cron_jobs: build_cron_capability_support(
            cron_registered,
            cron_enabled,
            cron_executable,
            plugin_active,
        ),
        tasks: build_registration_only_capability_support(
            !snapshot.tasks.is_empty(),
            plugin_active,
        ),
    }
}

fn build_tool_capability_support(
    snapshot: &crate::PluginCapabilitySnapshot,
    plugin_active: bool,
) -> PluginCapabilitySupportState {
    let registered = !snapshot.tools.is_empty();
    let executable = plugin_active && snapshot.tools.iter().any(|tool| tool.active);
    let any_active = snapshot.tools.iter().any(|tool| tool.active);
    let status = if !registered {
        "unsupported"
    } else if executable {
        "active"
    } else if plugin_active && !any_active {
        "disabled"
    } else {
        "disabled"
    };
    PluginCapabilitySupportState {
        registered,
        executable,
        persistent: false,
        active: executable,
        status: status.to_string(),
    }
}

fn build_web_api_capability_support(
    snapshot: &crate::PluginCapabilitySnapshot,
    plugin_active: bool,
) -> PluginCapabilitySupportState {
    let registered = !snapshot.web_apis.is_empty();
    let executable = registered && plugin_active;
    let status = if !registered {
        "unsupported"
    } else if executable {
        "active"
    } else {
        "disabled"
    };
    PluginCapabilitySupportState {
        registered,
        executable,
        persistent: false,
        active: executable,
        status: status.to_string(),
    }
}

fn build_registration_only_capability_support(
    registered: bool,
    plugin_active: bool,
) -> PluginCapabilitySupportState {
    let status = if !registered {
        "unsupported"
    } else if plugin_active {
        "registered_only"
    } else {
        "disabled"
    };
    PluginCapabilitySupportState {
        registered,
        executable: false,
        persistent: false,
        active: registered && plugin_active,
        status: status.to_string(),
    }
}

fn build_cron_capability_support(
    registered: bool,
    enabled: bool,
    executable: bool,
    plugin_active: bool,
) -> PluginCapabilitySupportState {
    let status = if !registered {
        "unsupported"
    } else if !plugin_active || !enabled {
        "disabled"
    } else if executable {
        "active"
    } else {
        "registered_only"
    };
    PluginCapabilitySupportState {
        registered,
        executable,
        persistent: executable,
        active: registered && plugin_active && enabled,
        status: status.to_string(),
    }
}

fn plugin_runtime_state_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginRuntimeStatePayload, String> {
    let payload = plugin_capabilities_payload(service, plugin_id)?;
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "plugin runtime host is unavailable".to_string())?;
    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let entry = catalog
        .entries
        .into_iter()
        .find(|entry| entry.descriptor.metadata.id == plugin_id)
        .ok_or_else(|| "plugin not found".to_string())?;
    let enabled = !catalog.disabled_plugin_ids.iter().any(|id| id == plugin_id);
    let active = entry.loaded && enabled;
    let executable_bindings = build_runtime_binding_summary(&payload.support);

    Ok(PluginRuntimeStatePayload {
        plugin_id: plugin_id.to_string(),
        runtime_kind: payload.runtime_kind,
        loaded: entry.loaded,
        enabled,
        active,
        snapshot_extracted: !payload.snapshot.tools.is_empty()
            || !payload.snapshot.web_apis.is_empty()
            || !payload.snapshot.cron_jobs.is_empty()
            || !payload.snapshot.tasks.is_empty(),
        executable_bindings,
        scheduler_status: if !enabled && payload.support.cron_jobs.registered {
            "disabled".to_string()
        } else {
            run_async_for_web_host(runtime_host.plugin_cron_scheduler_status(plugin_id))
                .unwrap_or_else(|_| "unsupported".to_string())
        },
        task_runtime_status: if payload.support.tasks.registered {
            "registered_only".to_string()
        } else {
            "unsupported".to_string()
        },
    })
}

fn plugin_diagnostics_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginDiagnosticsPayload, String> {
    let payload = plugin_capabilities_payload(service, plugin_id)?;
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "plugin runtime host is unavailable".to_string())?;
    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let entry = catalog
        .entries
        .into_iter()
        .find(|entry| entry.descriptor.metadata.id == plugin_id)
        .ok_or_else(|| "plugin not found".to_string())?;
    let diagnostics = run_async_for_web_host(runtime_host.plugin_runtime_diagnostics(plugin_id))?
        .unwrap_or_else(|| crate::PluginRuntimeDiagnostics {
            plugin_id: plugin_id.to_string(),
            ..crate::PluginRuntimeDiagnostics::default()
        });

    Ok(PluginDiagnosticsPayload {
        plugin_id: plugin_id.to_string(),
        runtime_kind: payload.runtime_kind,
        load_state: if entry.loaded {
            "loaded".to_string()
        } else {
            "unloaded".to_string()
        },
        snapshot_extracted: !payload.snapshot.tools.is_empty()
            || !payload.snapshot.web_apis.is_empty()
            || !payload.snapshot.cron_jobs.is_empty()
            || !payload.snapshot.tasks.is_empty(),
        executable_bindings: build_runtime_binding_summary(&payload.support),
        scheduler_status: if !entry.loaded && payload.support.cron_jobs.registered {
            "disabled".to_string()
        } else {
            run_async_for_web_host(runtime_host.plugin_cron_scheduler_status(plugin_id))
                .unwrap_or_else(|_| "unsupported".to_string())
        },
        last_web_api_dispatch: diagnostics.last_web_api_dispatch,
        last_tool_execution: diagnostics.last_tool_execution,
        last_cron_execution: diagnostics.last_cron_execution,
    })
}

fn build_runtime_binding_summary(
    support: &PluginCapabilitySupportSummary,
) -> PluginRuntimeBindingSummary {
    PluginRuntimeBindingSummary {
        tools: support.tools.executable,
        web_apis: support.web_apis.executable,
        cron_jobs: support.cron_jobs.executable,
        tasks: support.tasks.executable,
    }
}

fn normalize_registered_web_api_route(route_rest: &str) -> String {
    let trimmed = route_rest.trim_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn plugin_runtime_tool_name(plugin_id: &str, tool_name: &str) -> String {
    format!("plugin::{}::{}", plugin_id.trim(), tool_name.trim())
}

fn http_status_line(status_code: u16) -> String {
    format!("{status_code} {}", http_reason_phrase(status_code))
}

fn http_reason_phrase(status_code: u16) -> &'static str {
    match status_code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        412 => "Precondition Failed",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "OK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_capability_support_marks_disabled_jobs_as_disabled() {
        let support = build_cron_capability_support(true, false, false, true);
        assert_eq!(support.status, "disabled");
        assert!(!support.active);
        assert!(!support.executable);
    }

    #[test]
    fn cron_capability_support_marks_enabled_non_executable_jobs_as_registered_only() {
        let support = build_cron_capability_support(true, true, false, true);
        assert_eq!(support.status, "registered_only");
        assert!(support.active);
        assert!(!support.executable);
    }
}
