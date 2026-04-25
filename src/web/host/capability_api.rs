use super::*;

use crate::llm::tools::ToolManager;

pub(super) fn route_capability_api(method: &str, api_path: &str, is_head: bool) -> Option<Vec<u8>> {
    if api_path == "/tools" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "tools only accepts GET");
            return Some(napcat_response(body, is_head));
        }

        let body = match tools_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/mcp/servers" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "mcp/servers only accepts GET");
            return Some(napcat_response(body, is_head));
        }

        let body = match mcp_servers_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/skills" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "skills only accepts GET");
            return Some(napcat_response(body, is_head));
        }

        let body = match skills_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    None
}

fn tools_payload() -> Result<serde_json::Value, String> {
    let manager = ToolManager::for_current_workspace()?;
    let runtime = run_async_for_web_host(manager.describe_runtime_tools());
    Ok(serde_json::json!({
        "tools": runtime.tools,
        "warnings": runtime.warnings,
    }))
}

fn mcp_servers_payload() -> Result<serde_json::Value, String> {
    let manager = ToolManager::for_current_workspace()?;
    let snapshot = run_async_for_web_host(manager.describe_mcp_servers());
    Ok(serde_json::json!({
        "servers": snapshot.servers,
        "warnings": snapshot.warnings,
    }))
}

fn skills_payload() -> Result<serde_json::Value, String> {
    let manager = ToolManager::for_current_workspace()?;
    let snapshot = manager.describe_skills();
    Ok(serde_json::json!({
        "skills": snapshot.skills,
        "warnings": snapshot.warnings,
    }))
}
