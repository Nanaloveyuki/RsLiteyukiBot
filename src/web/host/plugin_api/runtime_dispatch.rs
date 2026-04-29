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

fn normalize_registered_web_api_route(route_rest: &str) -> String {
    let trimmed = route_rest.trim_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}")
    }
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
