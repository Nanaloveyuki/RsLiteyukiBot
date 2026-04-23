use super::*;

pub(super) fn route_webui_config_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    request: &[u8],
    raw_path: &str,
    is_head: bool,
    peer_ip: IpAddr,
) -> Option<Vec<u8>> {
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

    if api_path == "/NapCatConfig/GetConfig" || api_path == "/NapCatConfig/GetUinConfig" {
        let config = load_napcat_config(api_path == "/NapCatConfig/GetUinConfig");
        let body = napcat_ok(&config);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/NapCatConfig/SetConfig" || api_path == "/NapCatConfig/SetUinConfig" {
        let body = parse_json_body(request);
        if let Ok(config) = serde_json::from_value::<NapCatConfig>(body) {
            let _ = save_napcat_config(api_path == "/NapCatConfig/SetUinConfig", &config);
        }
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/WebUIConfig/GetConfig" {
        let config = load_webui_server_config(service.bind_addr.port());
        let body = napcat_ok(&config);
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
