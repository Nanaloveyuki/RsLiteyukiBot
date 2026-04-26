use super::*;

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::config_paths::resolve_preferred_mcp_config_path;
use crate::llm::mcp::{McpManager, McpServerConfig};
use crate::llm::skills::SkillManager;
use crate::llm::tools::ToolManager;

const NON_TOGGLEABLE_TOOLS: &[&str] = &[
    "list_tool_categories",
    "list_tools_in_category",
    "get_tool_schema",
];

pub(super) fn route_capability_api(
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
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

    if api_path == "/tools/toggle" {
        if !method.eq_ignore_ascii_case("POST") {
            let body = napcat_err(-1, "tools/toggle only accepts POST");
            return Some(napcat_response(body, is_head));
        }

        let body = match toggle_tool_payload(request) {
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

    if api_path == "/mcp/save" {
        if !method.eq_ignore_ascii_case("POST") {
            let body = napcat_err(-1, "mcp/save only accepts POST");
            return Some(napcat_response(body, is_head));
        }

        let body = match save_mcp_servers_payload(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/mcp/test" {
        if !method.eq_ignore_ascii_case("POST") {
            let body = napcat_err(-1, "mcp/test only accepts POST");
            return Some(napcat_response(body, is_head));
        }

        let body = match test_mcp_servers_payload(request) {
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

    if api_path == "/skills/read" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "skills/read only accepts GET");
            return Some(napcat_response(body, is_head));
        }

        let body = match read_skill_payload(raw_path) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/skills/upload" {
        if !method.eq_ignore_ascii_case("POST") {
            let body = napcat_err(-1, "skills/upload only accepts POST");
            return Some(napcat_response(body, is_head));
        }

        let body = match upload_skill_payload(request) {
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

fn toggle_tool_payload(request: &[u8]) -> Result<serde_json::Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebToolToggleRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid tools/toggle payload: {err}"))?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err("tool name should not be empty".to_string());
    }
    if NON_TOGGLEABLE_TOOLS.iter().any(|tool_name| tool_name == &name) {
        return Err(format!("tool '{name}' is a required discovery helper and cannot be toggled"));
    }

    let _guard = tool_toggle_lock()
        .lock()
        .map_err(|_| "tool toggle lock poisoned".to_string())?;
    let mut manager = ToolManager::for_current_workspace()?;
    let known_tools = run_async_for_web_host(manager.describe_runtime_tools());
    if !known_tools.tools.iter().any(|tool| tool.name == name) {
        return Err(format!("tool '{name}' was not found"));
    }

    manager.set_tool_active(name, payload.active)?;
    let runtime = run_async_for_web_host(manager.describe_runtime_tools());

    Ok(serde_json::json!({
        "name": name,
        "active": payload.active,
        "configPath": manager.tool_state_path().display().to_string(),
        "tools": runtime.tools,
        "warnings": runtime.warnings,
    }))
}

fn mcp_servers_payload() -> Result<serde_json::Value, String> {
    let manager = ToolManager::for_current_workspace()?;
    let snapshot = run_async_for_web_host(manager.describe_mcp_servers());
    Ok(serde_json::json!({
        "configPath": resolve_preferred_mcp_config_path().display().to_string(),
        "servers": snapshot.servers,
        "warnings": snapshot.warnings,
    }))
}

fn save_mcp_servers_payload(request: &[u8]) -> Result<serde_json::Value, String> {
    let servers = parse_mcp_server_list_request(request)?;
    let config_path = resolve_preferred_mcp_config_path();
    if let Some(parent) = config_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create MCP config directory: {err}"))?;
    }
    let content = serde_json::to_string_pretty(&serde_json::json!({ "servers": servers }))
        .map_err(|err| format!("failed to serialize MCP config: {err}"))?;
    fs::write(&config_path, content).map_err(|err| {
        format!(
            "failed to write MCP config {}: {err}",
            config_path.display()
        )
    })?;

    let snapshot =
        run_async_for_web_host(McpManager::from_config_path(&config_path).inspect_servers());
    Ok(serde_json::json!({
        "configPath": config_path.display().to_string(),
        "servers": snapshot.servers,
        "warnings": snapshot.warnings,
    }))
}

fn test_mcp_servers_payload(request: &[u8]) -> Result<serde_json::Value, String> {
    let servers = parse_mcp_server_test_request(request)?;
    let temp_path = std::env::temp_dir().join(format!(
        "liteyuki-web-mcp-test-{}.json",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let content = serde_json::to_string_pretty(&serde_json::json!({ "servers": servers }))
        .map_err(|err| format!("failed to serialize MCP test payload: {err}"))?;
    fs::write(&temp_path, content).map_err(|err| {
        format!(
            "failed to write MCP test config {}: {err}",
            temp_path.display()
        )
    })?;
    let snapshot =
        run_async_for_web_host(McpManager::from_config_path(&temp_path).inspect_servers());
    let _ = fs::remove_file(&temp_path);

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

fn read_skill_payload(raw_path: &str) -> Result<serde_json::Value, String> {
    let query = parse_query_string(raw_path);
    let skill_name = query
        .get("name")
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    if skill_name.is_empty() {
        return Err("missing skill name".to_string());
    }

    let max_chars = query
        .get("maxChars")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16_000)
        .clamp(256, 64_000);
    let manager = SkillManager::for_workspace(capability_workspace_root().as_path());
    let skill = resolve_skill_info(&manager, skill_name)?;
    let content = fs::read_to_string(&skill.entry_path)
        .map_err(|err| format!("failed to read {}: {err}", skill.entry_path.display()))?;
    let (content, truncated) = truncate_chars(content.as_str(), max_chars);

    Ok(serde_json::json!({
        "name": skill.name,
        "description": skill.description,
        "path": display_workspace_relative_path(skill.entry_path.as_path()),
        "content": content,
        "truncated": truncated,
    }))
}

fn upload_skill_payload(request: &[u8]) -> Result<serde_json::Value, String> {
    let request_body = parse_json_body(request);
    let payload: WebSkillUploadRequest = serde_json::from_value(request_body)
        .map_err(|err| format!("invalid skills/upload payload: {err}"))?;
    let skill_name = normalize_skill_name(payload.name.as_str())?;
    let content = payload.content.trim().to_string();
    if content.is_empty() {
        return Err("skill content should not be empty".to_string());
    }

    let root = capability_workspace_root();
    let skill_dir = root.join("skills").join(skill_name.as_str());
    let entry_path = skill_dir.join("SKILL.md");
    if entry_path.exists() && !payload.overwrite {
        return Err(format!("skill '{}' already exists", skill_name));
    }
    fs::create_dir_all(&skill_dir).map_err(|err| {
        format!(
            "failed to create skill directory {}: {err}",
            skill_dir.display()
        )
    })?;
    fs::write(&entry_path, format!("{}\n", content))
        .map_err(|err| format!("failed to write skill file {}: {err}", entry_path.display()))?;

    let manager = SkillManager::for_workspace(root.as_path());
    let skill = resolve_skill_info(&manager, skill_name.as_str())?;
    Ok(serde_json::json!({
        "name": skill.name,
        "description": skill.description,
        "path": display_workspace_relative_path(skill.entry_path.as_path()),
    }))
}

fn parse_mcp_server_list_request(request: &[u8]) -> Result<Vec<McpServerConfig>, String> {
    let body = parse_json_body(request);
    parse_mcp_server_configs_from_value(body, "invalid mcp/save payload")
}

fn parse_mcp_server_test_request(request: &[u8]) -> Result<Vec<McpServerConfig>, String> {
    let body = parse_json_body(request);
    if let Some(server) = body.get("server") {
        let config = serde_json::from_value::<McpServerConfig>(server.clone())
            .map_err(|err| format!("invalid mcp/test payload: {err}"))?;
        return Ok(vec![config]);
    }
    parse_mcp_server_configs_from_value(body, "invalid mcp/test payload")
}

fn parse_mcp_server_configs_from_value(
    body: serde_json::Value,
    error_prefix: &str,
) -> Result<Vec<McpServerConfig>, String> {
    let servers = if body.is_array() {
        serde_json::from_value::<Vec<McpServerConfig>>(body)
            .map_err(|err| format!("{error_prefix}: {err}"))?
    } else if let Some(value) = body.get("servers") {
        serde_json::from_value::<Vec<McpServerConfig>>(value.clone())
            .map_err(|err| format!("{error_prefix}: {err}"))?
    } else {
        return Err(format!("{error_prefix}: missing servers"));
    };

    if servers.is_empty() {
        return Err("at least one MCP server is required".to_string());
    }
    Ok(servers)
}

fn resolve_skill_info(
    manager: &SkillManager,
    skill_name: &str,
) -> Result<crate::llm::skills::SkillInfo, String> {
    manager
        .list_skills()?
        .into_iter()
        .find(|skill| skill.name == skill_name)
        .ok_or_else(|| format!("skill '{}' was not found", skill_name))
}

fn normalize_skill_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("skill name should not be empty".to_string());
    }
    if trimmed.contains(['/', '\\']) {
        return Err("skill name should not contain path separators".to_string());
    }
    let mut normalized = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            normalized.push(ch);
        } else if ch.is_whitespace() {
            normalized.push('-');
        } else {
            return Err("skill name should use letters, numbers, '-' or '_'".to_string());
        }
    }
    let normalized = normalized.trim_matches('-').trim_matches('_').to_string();
    if normalized.is_empty() {
        return Err("skill name should not be empty".to_string());
    }
    Ok(normalized)
}

fn display_workspace_relative_path(path: &std::path::Path) -> String {
    path.strip_prefix(capability_workspace_root())
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn capability_workspace_root() -> PathBuf {
    std::env::var_os("LY_WORKSPACE_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| fs::canonicalize(&path).unwrap_or(path))
        .unwrap_or_else(workspace_root)
}

fn tool_toggle_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    if value.chars().count() <= max_chars {
        return (value.to_string(), false);
    }

    let mut end = value.len();
    let mut count = 0;
    for (index, ch) in value.char_indices() {
        count += 1;
        if count > max_chars {
            end = index;
            break;
        }
        end = index + ch.len_utf8();
    }
    (value[..end].to_string(), true)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebSkillUploadRequest {
    name: String,
    content: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebToolToggleRequest {
    name: String,
    active: bool,
}
