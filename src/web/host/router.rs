use super::*;

pub(super) fn route_http_request(
    service: &WebHostService,
    request: &[u8],
    peer_ip: IpAddr,
) -> Vec<u8> {
    let request_line = String::from_utf8_lossy(request);
    let Some((method, raw_path)) = parse_request_line(&request_line) else {
        return build_response(
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"bad request",
            false,
        );
    };
    let path = raw_path.split('?').next().unwrap_or(raw_path);
    let is_head = method.eq_ignore_ascii_case("HEAD");

    if method.eq_ignore_ascii_case("OPTIONS") {
        return options_response();
    }

    if path == HEALTH_ROUTE {
        let body = serde_json::to_vec_pretty(&service.health())
            .unwrap_or_else(|_| b"{\"status\":\"serialization-error\"}".to_vec());
        return build_response("200 OK", "application/json; charset=utf-8", &body, is_head);
    }

    if path.starts_with("/api/")
        && !public_api_path(path)
        && !request_is_authorized(&service.auth, request)
    {
        return unauthorized_response(is_head);
    }

    if path == LOGS_ROUTE {
        let body = serde_json::to_vec_pretty(&WebHostLogsPayload {
            entries: recent_buffered_logs(LOGS_ROUTE_LIMIT),
        })
        .unwrap_or_else(|_| b"{\"entries\":[]}".to_vec());
        return build_response("200 OK", "application/json; charset=utf-8", &body, is_head);
    }

    if path == I18N_ROUTE {
        let body = serde_json::to_vec_pretty(&current_i18n_snapshot())
            .unwrap_or_else(|_| b"{\"messages\":{}}".to_vec());
        return build_response("200 OK", "application/json; charset=utf-8", &body, is_head);
    }

    if path == "/files/theme.css" {
        let body = render_theme_css(&load_theme_config());
        return build_response(
            "200 OK",
            "text/css; charset=utf-8",
            body.as_bytes(),
            is_head,
        );
    }

    if let Some(asset) = built_in_public_font(path) {
        return build_response("200 OK", asset.content_type(), asset.body(), is_head);
    }

    if path.starts_with("/plugin/") {
        return plugin_pages::route_plugin_page(service.runtime_host.as_ref(), path, is_head);
    }

    if let Some(asset) = service.assets.static_asset_for_path(path) {
        return build_response("200 OK", asset.content_type(), asset.body(), is_head);
    }

    if path.starts_with("/api/") {
        return service.route_napcat_api(request, method, path, raw_path, is_head, peer_ip);
    }

    if let Some(location) = dev_frontend_redirect(service, raw_path, path, &request_line) {
        return build_redirect_response("307 Temporary Redirect", location.as_str(), is_head);
    }

    match service.assets.asset_for_path(path) {
        Some(asset) => build_response("200 OK", asset.content_type(), asset.body(), is_head),
        None => build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            is_head,
        ),
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn route_napcat_api(
    service: &WebHostService,
    request: &[u8],
    method: &str,
    path: &str,
    raw_path: &str,
    is_head: bool,
    peer_ip: IpAddr,
) -> Vec<u8> {
    if path == "/files/theme.css" {
        let body = render_theme_css(&load_theme_config());
        return build_response(
            "200 OK",
            "text/css; charset=utf-8",
            body.as_bytes(),
            is_head,
        );
    }

    let api_path = path.strip_prefix("/api").unwrap_or(path);

    if api_path == "/auth/check" {
        let token = bearer_token_from_request(request);
        let body = if let Some(token) = token {
            if service.auth.is_session_token_valid(token.as_str()) {
                napcat_ok(&true)
            } else {
                napcat_err(401, "Unauthorized")
            }
        } else {
            napcat_ok(&false)
        };
        return napcat_response(body, is_head);
    }

    if let Some(response) = route_auth_api(&service.auth, api_path, request, peer_ip, is_head) {
        return response;
    }

    if let Some(response) =
        system_api::route_system_api(service, api_path, raw_path, request, is_head)
    {
        return response;
    }

    if let Some(response) = webui_config_api::route_webui_config_api(
        service, method, api_path, request, raw_path, is_head, peer_ip,
    ) {
        return response;
    }

    if let Some(response) =
        capability_api::route_capability_api(method, api_path, raw_path, request, is_head)
    {
        return response;
    }

    if let Some(response) = llm_api::route_llm_api(service, method, api_path, request, is_head) {
        return response;
    }

    if let Some(response) = log_api::route_log_api(service, api_path, raw_path, request, is_head) {
        return response;
    }

    if api_path.starts_with("/File/") {
        return file_api::route_file_api(method, api_path, raw_path, request, is_head);
    }

    if let Some(response) = plugin_api::route_plugin_runtime_web_api(
        service, method, api_path, raw_path, request, is_head, peer_ip,
    ) {
        return response;
    }

    if let Some(response) =
        plugin_api::route_plugin_api(service, method, api_path, raw_path, request, is_head)
    {
        return response;
    }

    if let Some(response) = mirror_api::route_mirror_api(api_path, raw_path, request, is_head) {
        return response;
    }

    if api_path == "/Debug/ws" || api_path == "/ws/terminal" {
        return build_response(
            "426 Upgrade Required",
            "text/plain; charset=utf-8",
            b"WebSocket upgrade required",
            is_head,
        );
    }

    let body = napcat_err(-1, "not found");
    napcat_response(body, is_head)
}

fn dev_frontend_redirect(
    service: &WebHostService,
    raw_path: &str,
    path: &str,
    request: &str,
) -> Option<String> {
    if path.starts_with("/api") {
        return None;
    }

    let dev_frontend = service.dev_frontend?;
    if !dev_frontend_is_available(dev_frontend) {
        return None;
    }

    let authority = request
        .lines()
        .find_map(|line| parse_named_header(line, "Host"))
        .map(|host| rewrite_host_port(host, dev_frontend.public_port))
        .unwrap_or_else(|| format!("{}:{}", service.browser_ip, dev_frontend.public_port));

    Some(format!("http://{authority}{raw_path}"))
}

fn dev_frontend_is_available(dev_frontend: WebHostDevServer) -> bool {
    StdTcpStream::connect_timeout(&dev_frontend.probe_addr, DEV_FRONTEND_PROBE_TIMEOUT).is_ok()
}

fn rewrite_host_port(host: &str, port: u16) -> String {
    if let Some(stripped) = host.strip_prefix('[')
        && let Some((address, _)) = stripped.split_once("]:")
    {
        return format!("[{address}]:{port}");
    }

    if host.starts_with('[') && host.ends_with(']') {
        return format!("{host}:{port}");
    }

    if let Some((hostname, _)) = host.rsplit_once(':')
        && !hostname.contains(':')
    {
        return format!("{hostname}:{port}");
    }

    format!("{host}:{port}")
}
