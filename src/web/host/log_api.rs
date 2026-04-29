use super::*;

pub(super) fn route_log_api(
    service: &WebHostService,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/Log/GetLogList" {
        let body = napcat_ok(&vec!["runtime.log".to_string()]);
        return Some(napcat_response(body, is_head));
    }

    if api_path.starts_with("/Log/GetLog") && !api_path.contains("RealTime") {
        let query = parse_query_string(raw_path);
        let log_id = query.get("id").cloned().unwrap_or_default();
        let body = napcat_ok(&if log_id.is_empty() || log_id == "runtime.log" {
            format_log_history(LOGS_ROUTE_LIMIT.max(400))
        } else {
            String::new()
        });
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Log/GetLogRealTime" {
        let entries = recent_buffered_logs(50);
        let event_data = if let Some(last) = entries.last() {
            serde_json::json!({
                "level": last.level,
                "message": last.message
            })
            .to_string()
        } else {
            serde_json::json!({ "level": "info", "message": "Liteyuki running" }).to_string()
        };
        return Some(sse_response(&event_data, is_head));
    }

    if api_path == "/Log/terminal/create" {
        let body = parse_json_body(request);
        let cols = body
            .get("cols")
            .and_then(Value::as_u64)
            .map(|value| value as u16)
            .unwrap_or(TERMINAL_DEFAULT_COLS);
        let rows = body
            .get("rows")
            .and_then(Value::as_u64)
            .map(|value| value as u16)
            .unwrap_or(TERMINAL_DEFAULT_ROWS);
        let id = service.terminal_state.create_session(cols, rows);
        let body = napcat_ok(&serde_json::json!({ "id": id }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Log/terminal/list" {
        let body = napcat_ok(
            &service
                .terminal_state
                .list_sessions()
                .into_iter()
                .map(|id| serde_json::json!({ "id": id }))
                .collect::<Vec<_>>(),
        );
        return Some(napcat_response(body, is_head));
    }

    if api_path.starts_with("/Log/terminal/") && api_path.ends_with("/close") {
        let terminal_id = api_path
            .strip_prefix("/Log/terminal/")
            .and_then(|value| value.strip_suffix("/close"))
            .unwrap_or_default();
        let closed = service.terminal_state.close_session(terminal_id);
        let body = napcat_ok(&closed);
        return Some(napcat_response(body, is_head));
    }

    None
}
