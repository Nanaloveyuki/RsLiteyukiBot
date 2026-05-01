use super::*;

#[test]
// 必要测试
fn merge_json_objects_preserves_unspecified_fields() {
    let base = serde_json::json!({
        "fileLog": true,
        "bypass": {
            "hook": true,
            "window": false
        }
    });
    let patch = serde_json::json!({
        "bypass": {
            "window": true
        }
    });

    let merged = merge_json_objects(base, patch);

    assert_eq!(merged["fileLog"], Value::Bool(true));
    assert_eq!(merged["bypass"]["hook"], Value::Bool(true));
    assert_eq!(merged["bypass"]["window"], Value::Bool(true));
}

#[test]
// 必要测试
fn parse_webui_appearance_update_keeps_only_image_data_urls() {
    let current = WebUiAppearanceConfigDoc {
        background_image: "data:image/png;base64,old".to_string(),
        custom_icons: HashMap::from([(
            "dashboard".to_string(),
            "data:image/png;base64,icon".to_string(),
        )]),
    };
    let request = b"POST /api/WebUIConfig/UpdateAppearance HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\r\n{\"backgroundImage\":\"\",\"customIcons\":{\"dashboard\":\"data:image/png;base64,new\",\"tools\":\"/assets/icon.png\"}}";

    let next = parse_webui_appearance_update(&current, request).expect("payload should parse");

    assert!(next.background_image.is_empty());
    assert_eq!(
        next.custom_icons.get("dashboard").map(String::as_str),
        Some("data:image/png;base64,new")
    );
    assert!(!next.custom_icons.contains_key("tools"));
}

#[test]
// 必要测试
fn validate_active_app_config_content_rejects_invalid_yaml() {
    let path = PathBuf::from("config.yaml");
    let err = validate_active_app_config_content(path.as_path(), "rust:\n  runtime: [")
        .expect_err("invalid yaml should be rejected");
    assert!(err.contains("invalid YAML config"));
}

#[test]
// 必要测试
fn validate_active_app_config_content_rejects_unknown_only_config() {
    let path = PathBuf::from("config.yaml");
    let err = validate_active_app_config_content(path.as_path(), "unknown:\n  value: true\n")
        .expect_err("unknown-only config should be rejected");
    assert!(err.contains("recognized top-level settings"));
}

#[test]
// 必要测试
fn parse_webui_appearance_update_rejects_invalid_json() {
    let current = WebUiAppearanceConfigDoc::default();
    let request = b"POST /api/WebUIConfig/UpdateAppearance HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\r\n{\"backgroundImage\":";

    let err = parse_webui_appearance_update(&current, request)
        .expect_err("invalid payload should be rejected");

    assert!(err.contains("invalid WebUIConfig/UpdateAppearance payload"));
}

#[test]
// 必要测试
fn reject_non_post_method_returns_method_error_response() {
    let response =
        reject_non_post_method("GET", "AppConfig/ReplaceActive", false).expect("response");
    let text = String::from_utf8(response).expect("response should be utf8");
    assert!(text.contains("AppConfig/ReplaceActive only accepts POST"));
    assert!(reject_non_post_method("POST", "AppConfig/ReplaceActive", false).is_none());
}

#[test]
fn normalize_onebot_config_payload_coerces_numeric_strings() {
    let normalized = normalize_onebot_config_payload(serde_json::json!({
        "network": {
            "websocketServers": [
                {
                    "port": "3001",
                    "heartInterval": "15000"
                }
            ],
            "websocketClients": [
                {
                    "reconnectInterval": "5000",
                    "heartInterval": "30000"
                }
            ],
            "httpServers": [
                {
                    "port": "8080"
                }
            ]
        },
        "timeout": {
            "baseTimeout": "10000",
            "uploadSpeedKBps": "1024",
            "downloadSpeedKBps": "1024",
            "maxTimeout": "60000"
        }
    }));

    assert_eq!(normalized["network"]["websocketServers"][0]["port"], 3001);
    assert_eq!(
        normalized["network"]["websocketServers"][0]["heartInterval"],
        15000
    );
    assert_eq!(
        normalized["network"]["websocketClients"][0]["reconnectInterval"],
        5000
    );
    assert_eq!(normalized["network"]["httpServers"][0]["port"], 8080);
    assert_eq!(normalized["timeout"]["baseTimeout"], 10000);
    assert_eq!(normalized["timeout"]["maxTimeout"], 60000);
}
