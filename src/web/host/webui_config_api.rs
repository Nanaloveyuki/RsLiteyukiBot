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

#[cfg(test)]
#[path = "webui_config_api/tests.rs"]
mod tests;
