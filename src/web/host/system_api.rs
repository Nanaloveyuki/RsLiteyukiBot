use super::*;

pub(super) fn route_system_api(
    service: &WebHostService,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/base/GetNapCatVersion" {
        #[derive(Serialize)]
        struct PackageInfo {
            version: String,
            #[serde(rename = "buildTime")]
            build_time: String,
        }
        let body = napcat_ok(&PackageInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            build_time: option_env!("VERGEN_BUILD_TIMESTAMP")
                .unwrap_or("unknown")
                .to_string(),
        });
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/getLatestTag" {
        let body = napcat_ok(&env!("CARGO_PKG_VERSION"));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/getAllReleases" {
        #[derive(Serialize)]
        struct Pagination {
            page: u32,
            #[serde(rename = "pageSize")]
            page_size: u32,
            total: u32,
            #[serde(rename = "totalPages")]
            total_pages: u32,
        }
        #[derive(Serialize)]
        struct Releases {
            versions: Vec<serde_json::Value>,
            pagination: Pagination,
        }
        let body = napcat_ok(&Releases {
            versions: vec![],
            pagination: Pagination {
                page: 1,
                page_size: 20,
                total: 0,
                total_pages: 0,
            },
        });
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/getMirrors" {
        #[derive(Serialize)]
        struct Mirrors {
            mirrors: Vec<String>,
        }
        let body = napcat_ok(&Mirrors { mirrors: vec![] });
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/QQVersion" {
        let body = napcat_ok(&"N/A");
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/Theme" {
        let body = napcat_ok(&load_theme_config());
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/SetTheme" {
        let body = parse_json_body(request);
        let theme_value = body.get("theme").cloned().unwrap_or(body);
        let theme = serde_json::from_value::<ThemeConfigDoc>(theme_value)
            .unwrap_or_else(|_| load_theme_config());
        let result = save_theme_config(&theme).is_ok();
        let body = napcat_ok(&result);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/proxy" {
        let body = napcat_ok(&"{}");
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/GetHitokoto" {
        let body = match run_async_for_web_host(super::upstream::fetch_upstream_json(
            "https://hitokoto.152710.xyz/",
            None,
        )) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/GetGitHubRepoSnapshot" {
        let query = parse_query_string(raw_path);
        let owner = match super::upstream::sanitize_repo_component(query.get("owner"), "owner") {
            Ok(owner) => owner,
            Err(err) => {
                let body = napcat_err(-1, err.as_str());
                return Some(napcat_response(body, is_head));
            }
        };
        let repo = match super::upstream::sanitize_repo_component(query.get("repo"), "repo") {
            Ok(repo) => repo,
            Err(err) => {
                let body = napcat_err(-1, err.as_str());
                return Some(napcat_response(body, is_head));
            }
        };

        let body = match run_async_for_web_host(async {
            let base = format!("https://api.github.com/repos/{owner}/{repo}");
            let repo_url = base.clone();
            let releases_url = format!("{base}/releases");
            let pulls_url = format!("{base}/pulls");
            let contributors_url = format!("{base}/contributors");
            let (repo, releases, pulls, contributors) = tokio::join!(
                super::upstream::fetch_upstream_json(repo_url.as_str(), None),
                super::upstream::fetch_upstream_json(releases_url.as_str(), None),
                super::upstream::fetch_upstream_json(pulls_url.as_str(), None),
                super::upstream::fetch_upstream_json(contributors_url.as_str(), None)
            );
            Ok::<Value, String>(serde_json::json!({
                "repo": repo?,
                "releases": releases?,
                "pulls": pulls?,
                "contributors": contributors?
            }))
        }) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/GetGitHubReadme" {
        let query = parse_query_string(raw_path);
        let owner = match super::upstream::sanitize_repo_component(query.get("owner"), "owner") {
            Ok(owner) => owner,
            Err(err) => {
                let body = napcat_err(-1, err.as_str());
                return Some(napcat_response(body, is_head));
            }
        };
        let repo = match super::upstream::sanitize_repo_component(query.get("repo"), "repo") {
            Ok(repo) => repo,
            Err(err) => {
                let body = napcat_err(-1, err.as_str());
                return Some(napcat_response(body, is_head));
            }
        };
        let url = format!("https://api.github.com/repos/{owner}/{repo}/readme");
        let body = match run_async_for_web_host(super::upstream::fetch_upstream_text(
            url.as_str(),
            Some("application/vnd.github.v3.raw"),
        )) {
            Ok(content) => napcat_ok(&serde_json::json!({ "content": content })),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/GetNapCatFileHash" {
        let body = napcat_ok(&serde_json::json!({
            "hash": "",
            "file": "",
            "algorithm": "sha256"
        }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/GetSysStatusRealTime" {
        let snapshot = (service.snapshot_provider)();
        let status = napcat_system_status(&snapshot);
        let event_data = serde_json::to_string(&status).unwrap_or_default();
        return Some(sse_response(&event_data, is_head));
    }

    if api_path == "/Process/Restart" {
        let body = napcat_ok(&serde_json::json!({ "message": "restart requested" }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/UpdateNapCat/update" {
        let body = napcat_ok(&serde_json::json!({
            "message": "Update not supported in Liteyuki"
        }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/CheckLoginStatus" {
        let body = napcat_ok(&serde_json::json!({
            "isLogin": false,
            "isOffline": false,
            "qrcodeurl": ""
        }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/RefreshQRcode" {
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetQQLoginQrcode" {
        let body = napcat_ok(&serde_json::json!({ "qrcode": "" }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetQuickLoginList" {
        let body = napcat_ok(&Vec::<String>::new());
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetQuickLoginListNew" {
        let snapshot = (service.snapshot_provider)();
        let body = napcat_ok(&vec![serde_json::json!({
            "uin": "local-webui",
            "uid": "local-webui",
            "nickName": snapshot.app_name,
            "faceUrl": "",
            "facePath": "",
            "loginType": 1,
            "isQuickLogin": true,
            "isAutoLogin": false
        })]);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/SetQuickLogin" {
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetQQLoginInfo" {
        let snapshot = (service.snapshot_provider)();
        let body = napcat_ok(&serde_json::json!({
            "uid": "local-webui",
            "uin": "local-webui",
            "nick": snapshot.app_name,
            "avatarUrl": serde_json::Value::Null,
            "online": snapshot.status == "running"
        }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetQuickLoginQQ" {
        let body = napcat_ok(&"local-webui");
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/SetQuickLoginQQ" {
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/PasswordLogin"
        || api_path == "/QQLogin/CaptchaLogin"
        || api_path == "/QQLogin/NewDeviceLogin"
        || api_path == "/QQLogin/GetNewDeviceQRCode"
        || api_path == "/QQLogin/PollNewDeviceQR"
        || api_path == "/QQLogin/ResetDeviceID"
        || api_path == "/QQLogin/RestartNapCat"
        || api_path == "/QQLogin/SetDeviceGUID"
        || api_path == "/QQLogin/RestoreGUIDBackup"
        || api_path == "/QQLogin/ResetLinuxDeviceID"
    {
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetDeviceGUID" {
        let body = napcat_ok(&serde_json::json!({ "guid": "" }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetGUIDBackups" {
        let body = napcat_ok(&Vec::<String>::new());
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/CreateGUIDBackup" {
        let body = napcat_ok(&serde_json::json!({ "path": "" }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetPlatformInfo" {
        let body = napcat_ok(&serde_json::json!({ "platform": std::env::consts::OS }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetLinuxMAC" {
        let body = napcat_ok(&serde_json::json!({ "mac": "" }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/SetLinuxMAC" {
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetLinuxMachineId" {
        let body = napcat_ok(&serde_json::json!({ "machineId": "" }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/ComputeLinuxGUID" {
        let body = napcat_ok(&serde_json::json!({ "guid": "", "machineId": "", "mac": "" }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetLinuxMachineInfoBackups" {
        let body = napcat_ok(&Vec::<String>::new());
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/CreateLinuxMachineInfoBackup" {
        let body = napcat_ok(&serde_json::json!({ "path": "" }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/RestoreLinuxMachineInfoBackup" {
        let body = napcat_ok(&serde_json::Value::Null);
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/QQLogin/GetAllUsers" {
        let snapshot = (service.snapshot_provider)();
        let body = napcat_ok(&vec![serde_json::json!({
            "uin": "local-webui",
            "uid": "local-webui",
            "nick": snapshot.app_name,
            "avatarUrl": serde_json::Value::Null,
            "online": snapshot.status == "running"
        })]);
        return Some(napcat_response(body, is_head));
    }

    None
}
