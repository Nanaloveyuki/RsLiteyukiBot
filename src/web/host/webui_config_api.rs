use super::*;
use crate::config_edit::FlowLocalAgentConfigPatch;
use crate::utils::llm_config::{normalize_non_empty_string, normalize_provider_url};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FlowLocalAgentWebConfigPayload {
    enabled: bool,
    base_url: String,
    has_token: bool,
    token_preview: String,
    device_id: String,
    device_name: String,
    auto_connect: bool,
    allowed_tools: Vec<String>,
    workspace_root: String,
    command_timeout_seconds: u64,
    approval_policy: String,
    effective_device_id: String,
    config_path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FlowLocalAgentStatusPayload {
    connected: bool,
    reconnect_allowed: bool,
    last_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FlowLocalAgentLogsPayload {
    entries: Vec<BufferedLogEntry>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FlowLocalAgentDeviceCodePayload {
    device_code: String,
    user_code: String,
    verification_url: String,
    expires_in: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FlowLocalAgentDeviceCodePollPayload {
    status: String,
    has_token: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FlowLocalAgentDeviceCodeStartResponse {
    device_code: String,
    user_code: String,
    verification_url: String,
    expires_in: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FlowLocalAgentDeviceCodePollResponse {
    status: String,
    token: Option<String>,
}

pub(super) fn route_webui_config_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    request: &[u8],
    raw_path: &str,
    is_head: bool,
    peer_ip: IpAddr,
) -> Option<Vec<u8>> {
    if api_path == "/AppConfig/GetActive" {
        let body = match active_app_config_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/AppConfig/ReplaceActive" {
        if let Some(response) = reject_non_post_method(method, "AppConfig/ReplaceActive", is_head) {
            return Some(response);
        }
        let body = match replace_active_app_config(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/OB11Config/GetConfig" {
        let config = load_onebot_config();
        let body = napcat_ok(&config);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/OB11Config/SetConfig" {
        let body = parse_json_body(request);
        let config_value = body
            .get("config")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .or_else(|| body.get("config").cloned())
            .unwrap_or(Value::Null);
        let config_value = normalize_onebot_config_payload(config_value);
        let config = match serde_json::from_value::<OneBotConfig>(config_value) {
            Ok(config) => config,
            Err(err) => {
                let body = napcat_err(-1, format!("invalid OB11 config payload: {err}").as_str());
                return Some(napcat_response(body, is_head));
            }
        };
        if let Some(runtime_host) = &service.runtime_host {
            let next_adapters = match config.to_runtime_adapter_configs() {
                Ok(adapters) => adapters,
                Err(err) => {
                    let body = napcat_err(-1, err.as_str());
                    return Some(napcat_response(body, is_head));
                }
            };
            let previous_config = load_onebot_config();
            let previous_adapters = run_async_for_web_host(runtime_host.adapter_configs());
            if let Err(err) = save_onebot_config(&config) {
                let body = napcat_err(-1, err.as_str());
                return Some(napcat_response(body, is_head));
            }
            if let Err(err) =
                run_async_for_web_host(runtime_host.apply_adapter_configs(next_adapters))
            {
                let _ = save_onebot_config(&previous_config);
                let rollback_result =
                    run_async_for_web_host(runtime_host.apply_adapter_configs(previous_adapters));
                let message = match rollback_result {
                    Ok(()) => err,
                    Err(rollback_err) => format!("{err}; rollback failed: {rollback_err}"),
                };
                let body = napcat_err(-1, message.as_str());
                return Some(napcat_response(body, is_head));
            }
        } else if let Err(err) = save_onebot_config(&config) {
            let body = napcat_err(-1, err.as_str());
            return Some(napcat_response(body, is_head));
        }
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/RuntimeConfig/GetConfig"
        || api_path == "/RuntimeConfig/GetAccountConfig"
        || api_path == "/NapCatConfig/GetConfig"
        || api_path == "/NapCatConfig/GetUinConfig"
    {
        let config = load_napcat_config(
            api_path == "/RuntimeConfig/GetAccountConfig"
                || api_path == "/NapCatConfig/GetUinConfig",
        );
        let body = napcat_ok(&config);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/RuntimeConfig/SetConfig"
        || api_path == "/RuntimeConfig/SetAccountConfig"
        || api_path == "/NapCatConfig/SetConfig"
        || api_path == "/NapCatConfig/SetUinConfig"
    {
        let body = parse_json_body(request);
        let use_uin_config = api_path == "/RuntimeConfig/SetAccountConfig"
            || api_path == "/NapCatConfig/SetUinConfig";
        let current = load_napcat_config(use_uin_config);
        let current_value = serde_json::to_value(&current)
            .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
        let merged_value = merge_json_objects(current_value, body);
        let body = match serde_json::from_value::<NapCatConfig>(merged_value) {
            Ok(config) => match save_napcat_config(use_uin_config, &config) {
                Ok(()) => napcat_ok(&serde_json::Value::Null),
                Err(err) => napcat_err(-1, err.as_str()),
            },
            Err(err) => napcat_err(
                -1,
                format!("invalid runtime config payload: {err}").as_str(),
            ),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/GetConfig" {
        let config = load_webui_server_config(service.bind_addr.port());
        let body = napcat_ok(&config);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/GetConfig" {
        let body = match load_flow_local_agent_web_config_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/SetConfig" {
        if let Some(response) = reject_non_post_method(method, "FlowLocalAgent/SetConfig", is_head)
        {
            return Some(response);
        }
        let body = match save_flow_local_agent_web_config_with_runtime(service, request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/SetToken" {
        if let Some(response) = reject_non_post_method(method, "FlowLocalAgent/SetToken", is_head)
        {
            return Some(response);
        }
        let body = match save_flow_local_agent_token(service, request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/Auth/DeviceCode/Start" {
        if let Some(response) = reject_non_post_method(
            method,
            "FlowLocalAgent/Auth/DeviceCode/Start",
            is_head,
        ) {
            return Some(response);
        }
        let body = match start_flow_local_agent_device_code(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/Auth/DeviceCode/Poll" {
        if let Some(response) = reject_non_post_method(
            method,
            "FlowLocalAgent/Auth/DeviceCode/Poll",
            is_head,
        ) {
            return Some(response);
        }
        let body = match poll_flow_local_agent_device_code(service, request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/ConnectNow" {
        if let Some(response) = reject_non_post_method(method, "FlowLocalAgent/ConnectNow", is_head)
        {
            return Some(response);
        }
        let body = match restart_flow_local_agent_runtime(service) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/DisconnectNow" {
        if let Some(response) = reject_non_post_method(method, "FlowLocalAgent/DisconnectNow", is_head)
        {
            return Some(response);
        }
        let body = match disconnect_flow_local_agent_runtime(service) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/GetLogs" {
        let body = napcat_ok(&flow_local_agent_logs_payload());
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/FlowLocalAgent/GetStatus" {
        let body = napcat_ok(&flow_local_agent_status_payload(service));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/GetAppearance" {
        let body = napcat_ok(&load_webui_appearance_config());
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/UpdateAppearance" {
        if let Some(response) =
            reject_non_post_method(method, "WebUIConfig/UpdateAppearance", is_head)
        {
            return Some(response);
        }
        let body = match parse_webui_appearance_update(&load_webui_appearance_config(), request) {
            Ok(next) => match save_webui_appearance_config(&next) {
                Ok(()) => napcat_ok(&next),
                Err(err) => napcat_err(-1, err.as_str()),
            },
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/UpdateConfig" {
        let mut config = load_webui_server_config(service.bind_addr.port());
        let body = parse_json_body(request);
        if let Some(host) = body.get("host").and_then(Value::as_str) {
            config.host = host.trim().to_string();
        }
        if let Some(port) = body.get("port").and_then(Value::as_u64) {
            config.port = port as u16;
        }
        if let Some(login_rate) = body.get("loginRate").and_then(Value::as_u64) {
            config.login_rate = login_rate as u32;
        }
        if let Some(disable) = body.get("disableWebUI").and_then(Value::as_bool) {
            config.disable_webui = disable;
        }
        if let Some(mode) = body.get("accessControlMode").and_then(Value::as_str) {
            config.access_control_mode = mode.to_string();
        }
        if let Some(whitelist) = body.get("ipWhitelist").and_then(Value::as_array) {
            config.ip_whitelist = whitelist
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect();
        }
        if let Some(blacklist) = body.get("ipBlacklist").and_then(Value::as_array) {
            config.ip_blacklist = blacklist
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect();
        }
        if let Some(enabled) = body.get("enableXForwardedFor").and_then(Value::as_bool) {
            config.enable_x_forwarded_for = enabled;
        }
        let body = match save_webui_server_config(&config) {
            Ok(()) => napcat_ok(&true),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/GetDisableWebUI" {
        let body = napcat_ok(&load_webui_server_config(service.bind_addr.port()).disable_webui);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/UpdateDisableWebUI" {
        let mut config = load_webui_server_config(service.bind_addr.port());
        let body = if let Some(disable) = parse_json_body(request)
            .get("disable")
            .and_then(Value::as_bool)
        {
            config.disable_webui = disable;
            match save_webui_server_config(&config) {
                Ok(()) => napcat_ok(&true),
                Err(err) => napcat_err(-1, err.as_str()),
            }
        } else {
            napcat_err(-1, "missing disable flag")
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/GetClientIP" {
        let request_str = String::from_utf8_lossy(request);
        let config = load_webui_server_config(service.bind_addr.port());
        let ip = if config.enable_x_forwarded_for {
            extract_header(&request_str, "X-Forwarded-For")
                .unwrap_or("127.0.0.1")
                .to_string()
        } else {
            peer_ip.to_string()
        };
        let body = napcat_ok(&serde_json::json!({ "ip": ip }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Desktop/GetSettings" {
        let behavior = crate::app_config::resolve_desktop_close_behavior();
        let body = napcat_ok(&serde_json::json!({
            "closeToTray": behavior.close_to_tray,
            "configured": behavior.configured,
            "configPath": crate::app_config::resolve_app_config_path()
                .map(|path| path.display().to_string())
        }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Desktop/UpdateSettings" {
        let body = if let Some(close_to_tray) = parse_json_body(request)
            .get("closeToTray")
            .and_then(Value::as_bool)
        {
            match crate::app_config::persist_desktop_close_to_tray_preference(close_to_tray) {
                Ok(behavior) => napcat_ok(&serde_json::json!({
                    "closeToTray": behavior.close_to_tray,
                    "configured": behavior.configured,
                    "configPath": crate::app_config::resolve_app_config_path()
                        .map(|path| path.display().to_string())
                })),
                Err(err) => napcat_err(-1, err.as_str()),
            }
        } else {
            napcat_err(-1, "missing closeToTray flag")
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/GetSSLStatus" {
        let cert_path = state_path(SSL_CERT_FILE);
        let key_path = state_path(SSL_KEY_FILE);
        let cert_content = fs::read_to_string(&cert_path).unwrap_or_default();
        let key_content = fs::read_to_string(&key_path).unwrap_or_default();
        let body = napcat_ok(&serde_json::json!({
            "enabled": cert_path.is_file() && key_path.is_file(),
            "certExists": cert_path.is_file(),
            "keyExists": key_path.is_file(),
            "certContent": cert_content,
            "keyContent": key_content
        }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/UploadSSLCert" {
        let body = parse_json_body(request);
        let cert = body.get("cert").and_then(Value::as_str).unwrap_or_default();
        let key = body.get("key").and_then(Value::as_str).unwrap_or_default();
        let result = if cert.trim().is_empty() || key.trim().is_empty() {
            Err("certificate or private key is empty".to_string())
        } else {
            fs::create_dir_all(state_path(WEBUI_STATE_DIR))
                .map_err(|err| format!("failed to create webui state dir: {err}"))
                .and_then(|_| {
                    fs::write(state_path(SSL_CERT_FILE), cert)
                        .map_err(|err| format!("failed to write cert: {err}"))?;
                    fs::write(state_path(SSL_KEY_FILE), key)
                        .map_err(|err| format!("failed to write key: {err}"))
                })
        };
        let body = match result {
            Ok(()) => napcat_ok(&serde_json::json!({ "message": "SSL certificate saved" })),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/DeleteSSLCert" {
        let _ = fs::remove_file(state_path(SSL_CERT_FILE));
        let _ = fs::remove_file(state_path(SSL_KEY_FILE));
        let body = napcat_ok(&serde_json::json!({ "message": "SSL certificate deleted" }));
        return Some(napcat_response(body, is_head));
    }

    let _ = method;
    let _ = raw_path;
    None
}

fn reject_non_post_method(method: &str, route_name: &str, is_head: bool) -> Option<Vec<u8>> {
    if method.eq_ignore_ascii_case("POST") {
        return None;
    }

    let body = napcat_err(-1, format!("{route_name} only accepts POST").as_str());
    Some(napcat_response(body, is_head))
}

fn parse_webui_appearance_update(
    current: &WebUiAppearanceConfigDoc,
    request: &[u8],
) -> Result<WebUiAppearanceConfigDoc, String> {
    let body = parse_json_object_body(request, "WebUIConfig/UpdateAppearance")?;
    let mut next = current.clone();

    if let Some(background_image) = body.get("backgroundImage").and_then(Value::as_str) {
        next.background_image = normalize_webui_data_url(background_image).unwrap_or_default();
    }

    if let Some(custom_icons) = body.get("customIcons").and_then(Value::as_object) {
        next.custom_icons = custom_icons
            .iter()
            .filter_map(|(key, value)| {
                value
                    .as_str()
                    .and_then(normalize_webui_data_url)
                    .map(|icon| (key.to_string(), icon))
            })
            .collect();
    }

    Ok(next)
}

fn parse_json_object_body(
    request: &[u8],
    route_name: &str,
) -> Result<serde_json::Map<String, Value>, String> {
    let body = serde_json::from_slice::<Value>(extract_body(request))
        .map_err(|err| format!("invalid {route_name} payload: {err}"))?;
    body.as_object()
        .cloned()
        .ok_or_else(|| format!("{route_name} payload must be a JSON object"))
}

fn load_flow_local_agent_web_config_payload() -> Result<FlowLocalAgentWebConfigPayload, String> {
    crate::app_config::ensure_default_config_files().map_err(|err| err.to_string())?;
    let (doc, _) = crate::app_config::load_app_config_with_warnings(false);
    let runtime = crate::app_config::resolve_flow_local_agent_config(&doc);
    let token = runtime.token.clone().unwrap_or_default();
    let effective_device_id = crate::flow_local_agent::device::normalize_runtime_config(runtime.clone())
        .0
        .device_id
        .unwrap_or_default();
    let path = crate::app_config::resolve_app_config_path()
        .unwrap_or_else(crate::utils::config_path::resolve_default_app_config_path);

    Ok(FlowLocalAgentWebConfigPayload {
        enabled: runtime.enabled,
        base_url: runtime.base_url.unwrap_or_default(),
        has_token: !token.is_empty(),
        token_preview: preview_secret_token(token.as_str()),
        device_id: doc
            .flow_local_agent
            .as_ref()
            .and_then(|section| section.device_id.clone())
            .unwrap_or_default(),
        device_name: runtime.device_name.unwrap_or_default(),
        auto_connect: runtime.auto_connect,
        allowed_tools: runtime.allowed_tools,
        workspace_root: runtime
            .workspace_root
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        command_timeout_seconds: runtime.command_timeout_ms.saturating_div(1000).max(1),
        approval_policy: runtime.approval_policy,
        effective_device_id,
        config_path: path.display().to_string(),
    })
}

fn save_flow_local_agent_web_config(request: &[u8]) -> Result<Value, String> {
    let body = parse_json_object_body(request, "FlowLocalAgent/SetConfig")?;
    let patch = FlowLocalAgentConfigPatch {
        enabled: body.get("enabled").and_then(Value::as_bool),
        base_url: body
            .get("baseUrl")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        token: None,
        device_id: body
            .get("deviceId")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        device_name: body
            .get("deviceName")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        auto_connect: body.get("autoConnect").and_then(Value::as_bool),
        allowed_tools: body.get("allowedTools").and_then(|value| {
            value.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
        }),
        workspace_root: body
            .get("workspaceRoot")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        command_timeout_seconds: body
            .get("commandTimeoutSeconds")
            .and_then(value_as_u64_or_numeric_string),
        approval_policy: body
            .get("approvalPolicy")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    };
    let path = active_app_config_path()?;
    crate::config_edit::persist_flow_local_agent_config(path.as_path(), &patch)?;
    let payload = load_flow_local_agent_web_config_payload()?;
    serde_json::to_value(payload).map_err(|err| format!("failed to serialize flow local agent config: {err}"))
}

fn save_flow_local_agent_web_config_with_runtime(
    service: &WebHostService,
    request: &[u8],
) -> Result<Value, String> {
    let payload = save_flow_local_agent_web_config(request)?;
    let _ = restart_flow_local_agent_runtime(service);
    Ok(payload)
}

fn save_flow_local_agent_token(service: &WebHostService, request: &[u8]) -> Result<Value, String> {
    let body = parse_json_object_body(request, "FlowLocalAgent/SetToken")?;
    let token = body
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| "token is required".to_string())?;

    persist_flow_local_agent_token(token)?;
    let _ = restart_flow_local_agent_runtime(service);
    let payload = load_flow_local_agent_web_config_payload()?;
    serde_json::to_value(payload)
        .map_err(|err| format!("failed to serialize flow local agent token state: {err}"))
}

fn start_flow_local_agent_device_code(request: &[u8]) -> Result<Value, String> {
    let body = parse_json_object_body(request, "FlowLocalAgent/Auth/DeviceCode/Start")?;
    let base_url = resolve_flow_local_agent_base_url(body.get("baseUrl").and_then(Value::as_str))?;
    let request_payload = serde_json::json!({
        "server_url": base_url,
    });
    let url = format!("{base_url}/api/v1/auth/device/code");
    let response = run_async_for_web_host(async move {
        let response = super::upstream::upstream_http_client()
            .post(url)
            .json(&request_payload)
            .send()
            .await
            .map_err(|err| format!("failed to request flow local agent device code: {err}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| format!("failed to read flow local agent device code response: {err}"))?;
        if !status.is_success() {
            return Err(format!(
                "flow device auth returned {}: {}",
                status.as_u16(),
                summarize_brief_error_body(body.as_str())
            ));
        }
        serde_json::from_str::<FlowLocalAgentDeviceCodeStartResponse>(body.as_str())
            .map_err(|err| format!("invalid flow local agent device code response: {err}"))
    })?;
    serde_json::to_value(FlowLocalAgentDeviceCodePayload {
        device_code: response.device_code,
        user_code: response.user_code,
        verification_url: response.verification_url,
        expires_in: response.expires_in,
    })
    .map_err(|err| format!("failed to serialize flow local agent device code payload: {err}"))
}

fn poll_flow_local_agent_device_code(
    service: &WebHostService,
    request: &[u8],
) -> Result<Value, String> {
    let body = parse_json_object_body(request, "FlowLocalAgent/Auth/DeviceCode/Poll")?;
    let base_url = resolve_flow_local_agent_base_url(body.get("baseUrl").and_then(Value::as_str))?;
    let device_code = body
        .get("deviceCode")
        .and_then(Value::as_str)
        .and_then(normalize_non_empty_string)
        .ok_or_else(|| "deviceCode is required".to_string())?;

    let request_payload = serde_json::json!({
        "device_code": device_code,
    });
    let url = format!("{base_url}/api/v1/auth/device/token");
    let response = run_async_for_web_host(async move {
        let response = super::upstream::upstream_http_client()
            .post(url)
            .json(&request_payload)
            .send()
            .await
            .map_err(|err| format!("failed to poll flow local agent device token: {err}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| format!("failed to read flow local agent device token response: {err}"))?;
        if !status.is_success() {
            return Err(format!(
                "flow device token returned {}: {}",
                status.as_u16(),
                summarize_brief_error_body(body.as_str())
            ));
        }
        serde_json::from_str::<FlowLocalAgentDeviceCodePollResponse>(body.as_str())
            .map_err(|err| format!("invalid flow local agent device token response: {err}"))
    })?;

    let has_token = match response.token.as_deref() {
        Some(token) if response.status == "approved" => {
            persist_flow_local_agent_token(token)?;
            let _ = restart_flow_local_agent_runtime(service);
            true
        }
        _ => false,
    };

    serde_json::to_value(FlowLocalAgentDeviceCodePollPayload {
        status: response.status,
        has_token,
    })
    .map_err(|err| format!("failed to serialize flow local agent device poll payload: {err}"))
}

fn persist_flow_local_agent_token(token: &str) -> Result<(), String> {
    let path = active_app_config_path()?;
    crate::config_edit::persist_flow_local_agent_config(
        path.as_path(),
        &FlowLocalAgentConfigPatch {
            token: Some(token.to_string()),
            ..Default::default()
        },
    )
}

fn restart_flow_local_agent_runtime(service: &WebHostService) -> Result<Value, String> {
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "flow local agent runtime host is unavailable".to_string())?;
    runtime_host.restart_flow_local_agent()?;
    Ok(serde_json::json!({
        "requested": true,
    }))
}

fn disconnect_flow_local_agent_runtime(service: &WebHostService) -> Result<Value, String> {
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "flow local agent runtime host is unavailable".to_string())?;
    runtime_host.disconnect_flow_local_agent();
    Ok(serde_json::json!({
        "requested": true,
    }))
}

fn resolve_flow_local_agent_base_url(raw: Option<&str>) -> Result<String, String> {
    if let Some(value) = raw.and_then(normalize_provider_url) {
        return Ok(value);
    }

    crate::app_config::ensure_default_config_files().map_err(|err| err.to_string())?;
    let (doc, _) = crate::app_config::load_app_config_with_warnings(false);
    crate::app_config::resolve_flow_local_agent_config(&doc)
        .base_url
        .ok_or_else(|| "flow local agent baseUrl is required".to_string())
}

fn preview_secret_token(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() <= 12 {
        return format!("{}...", &trimmed[..trimmed.len().min(4)]);
    }
    let prefix = &trimmed[..4];
    let suffix = &trimmed[trimmed.len() - 4..];
    format!("{prefix}...{suffix}")
}

fn summarize_brief_error_body(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("detail")
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| body.trim().chars().take(160).collect::<String>())
}

fn flow_local_agent_status_payload(service: &WebHostService) -> FlowLocalAgentStatusPayload {
    let snapshot = service
        .flow_local_agent_state
        .as_ref()
        .map(crate::flow_local_agent::FlowLocalAgentRuntimeState::snapshot);

    FlowLocalAgentStatusPayload {
        connected: snapshot.as_ref().map(|value| value.connected).unwrap_or(false),
        reconnect_allowed: snapshot
            .as_ref()
            .map(|value| value.reconnect_allowed)
            .unwrap_or(false),
        last_error: snapshot.and_then(|value| value.last_error),
    }
}

fn flow_local_agent_logs_payload() -> FlowLocalAgentLogsPayload {
    let entries = recent_buffered_logs(400)
        .into_iter()
        .filter(|entry| is_flow_local_agent_log_module(entry.module.as_str()))
        .collect();

    FlowLocalAgentLogsPayload { entries }
}

fn is_flow_local_agent_log_module(module: &str) -> bool {
    module == "flow.local_agent"
        || module.starts_with("flow.local_agent.")
        || module == "flow.local_agent.tool"
        || module.starts_with("flow.local_agent.tool.")
}

fn normalize_webui_data_url(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }

    if value.starts_with("data:image/") {
        return Some(value.to_string());
    }

    if value.starts_with("http://") || value.starts_with("https://") || value.starts_with('/') {
        return None;
    }

    None
}

fn active_app_config_path() -> Result<PathBuf, String> {
    crate::app_config::ensure_default_config_files().map_err(|err| err.to_string())?;
    Ok(crate::app_config::resolve_app_config_path()
        .unwrap_or_else(crate::utils::config_path::resolve_default_app_config_path))
}

fn validate_active_app_config_content(path: &Path, content: &str) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());

    let doc = match ext.as_deref() {
        Some("yaml") | Some("yml") => {
            serde_yaml::from_str::<crate::app_config::AppConfigDoc>(content)
                .map_err(|err| format!("invalid YAML config: {err}"))?
        }
        Some("toml") => toml::from_str::<crate::app_config::AppConfigDoc>(content)
            .map_err(|err| format!("invalid TOML config: {err}"))?,
        _ => {
            return Err(format!(
                "unsupported config extension for {}",
                path.display()
            ));
        }
    };

    if !has_known_app_config_sections(&doc) {
        return Err("config does not contain any recognized top-level settings".to_string());
    }

    let warnings = crate::app_config::validate_app_config(&doc);
    if !warnings.is_empty() {
        return Err(format!("config validation failed: {}", warnings.join("; ")));
    }

    Ok(())
}

fn has_known_app_config_sections(doc: &crate::app_config::AppConfigDoc) -> bool {
    doc.rust.is_some()
        || doc.runtime.is_some()
        || doc.log.is_some()
        || doc.adapters.is_some()
        || doc.connect.is_some()
        || doc.tui.is_some()
        || doc.i18n.is_some()
        || doc.llm.is_some()
        || doc.flow_local_agent.is_some()
        || doc.commands.is_some()
        || doc.plugins.is_some()
        || doc.desktop.is_some()
        || doc.onebot_v11.is_some()
}

fn active_app_config_payload() -> Result<Value, String> {
    let path = active_app_config_path()?;
    let content = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read active config {}: {err}", path.display()))?;
    Ok(serde_json::json!({
        "configPath": path.display().to_string(),
        "content": content,
    }))
}

fn replace_active_app_config(request: &[u8]) -> Result<Value, String> {
    let body = parse_json_object_body(request, "AppConfig/ReplaceActive")?;
    let content = body
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "config content is required".to_string())?;
    let path = active_app_config_path()?;
    validate_active_app_config_content(path.as_path(), content)?;
    crate::config_edit::write_text_file_atomically(&path, content)
        .map_err(|err| format!("failed to write active config {}: {err}", path.display()))?;
    active_app_config_payload()
}

fn merge_json_objects(base: Value, patch: Value) -> Value {
    match (base, patch) {
        (Value::Object(mut base_map), Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                let next = match base_map.remove(&key) {
                    Some(existing) => merge_json_objects(existing, value),
                    None => value,
                };
                base_map.insert(key, next);
            }
            Value::Object(base_map)
        }
        (_, patch_value) => patch_value,
    }
}

fn normalize_onebot_config_payload(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            if let Some(network) = map.remove("network") {
                map.insert(
                    "network".to_string(),
                    normalize_onebot_network_payload(network),
                );
            }
            if let Some(timeout) = map.remove("timeout") {
                map.insert(
                    "timeout".to_string(),
                    normalize_onebot_timeout_payload(timeout),
                );
            }
            Value::Object(map)
        }
        other => other,
    }
}

fn normalize_onebot_network_payload(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            normalize_object_array_integer_fields(&mut map, "httpServers", &["port"]);
            normalize_object_array_integer_fields(
                &mut map,
                "websocketServers",
                &["port", "heartInterval"],
            );
            normalize_object_array_integer_fields(
                &mut map,
                "websocketClients",
                &["reconnectInterval", "heartInterval"],
            );
            Value::Object(map)
        }
        other => other,
    }
}

fn normalize_onebot_timeout_payload(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            normalize_integer_fields(
                &mut map,
                &[
                    "baseTimeout",
                    "uploadSpeedKBps",
                    "downloadSpeedKBps",
                    "maxTimeout",
                ],
            );
            Value::Object(map)
        }
        other => other,
    }
}

fn normalize_object_array_integer_fields(
    map: &mut serde_json::Map<String, Value>,
    key: &str,
    fields: &[&str],
) {
    let Some(Value::Array(items)) = map.get_mut(key) else {
        return;
    };

    for item in items {
        let Value::Object(item_map) = item else {
            continue;
        };
        normalize_integer_fields(item_map, fields);
    }
}

fn normalize_integer_fields(map: &mut serde_json::Map<String, Value>, fields: &[&str]) {
    for field in fields {
        let Some(value) = map.get_mut(*field) else {
            continue;
        };
        normalize_integer_value(value);
    }
}

fn normalize_integer_value(value: &mut Value) {
    let Some(raw) = value.as_str() else {
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Ok(parsed) = trimmed.parse::<u64>() {
        *value = Value::Number(parsed.into());
    }
}

fn value_as_u64_or_numeric_string(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_str()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
    })
}

#[cfg(test)]
#[path = "webui_config_api/tests.rs"]
mod tests;
