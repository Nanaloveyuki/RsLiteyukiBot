use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use liteyukibot_core::app_host::AppHostSnapshot;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};
use liteyukibot_core::web::host::{WebHostConfig, WebHostService};
use liteyukibot_core::web::ui::build_default_web_host_assets;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct ReleaseApiTestEnv {
    _lock: std::sync::MutexGuard<'static, ()>,
    root: PathBuf,
    _guards: Vec<EnvVarGuard>,
}

impl ReleaseApiTestEnv {
    fn new(api_base: &str) -> Self {
        let lock = process_state_lock();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rsliteyuki-release-api-{nanos}"));
        std::fs::create_dir_all(root.as_path()).expect("temp test root should be created");

        let guards = vec![
            EnvVarGuard::set("USERPROFILE", root.as_path()),
            EnvVarGuard::set("HOME", root.as_path()),
            EnvVarGuard::remove("LY_WEBUI_PASSWORD_PATH"),
            EnvVarGuard::set("LY_RELEASE_GITHUB_API_BASE", api_base),
        ];

        Self {
            _lock: lock,
            root,
            _guards: guards,
        }
    }
}

impl Drop for ReleaseApiTestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.root.as_path());
    }
}

async fn spawn_fake_github_api() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("fake github api listener should bind");
    let addr = listener
        .local_addr()
        .expect("fake github api listener should have local addr");
    let base = format!("http://{}", addr);

    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };

            let mut buffer = vec![0_u8; 8192];
            let Ok(read) = socket.read(&mut buffer).await else {
                continue;
            };
            if read == 0 {
                continue;
            }

            let request = String::from_utf8_lossy(&buffer[..read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");

            let body = if path.contains("/releases?") && path.contains("page=1") {
                release_fixture().to_string()
            } else {
                "[]".to_string()
            };

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });

    (base, task)
}

fn release_fixture() -> Value {
    json!([
      {
        "tag_name": "v0.1.0-c0001",
        "name": "Liteyuki v0.1.0-c0001",
        "html_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/tag/v0.1.0-c0001",
        "body": "older stable release",
        "draft": false,
        "prerelease": false,
        "created_at": "2026-04-20T09:04:40Z",
        "published_at": "2026-04-20T09:11:59Z",
        "assets": [
          {
            "name": "Liteyuki_0.1.0_x64_en-US.msi",
            "content_type": "application/x-msi",
            "size": 10301440,
            "download_count": 1,
            "browser_download_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/download/v0.1.0-c0001/Liteyuki_0.1.0_x64_en-US.msi",
            "created_at": "2026-04-20T09:17:32Z",
            "updated_at": "2026-04-20T09:17:33Z"
          }
        ]
      },
      {
        "tag_name": "nightly-build",
        "name": "Nightly",
        "html_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/tag/nightly-build",
        "body": "invalid version tag",
        "draft": false,
        "prerelease": false,
        "created_at": "2026-04-21T09:04:40Z",
        "published_at": "2026-04-21T09:11:59Z",
        "assets": []
      },
      {
        "tag_name": "v0.1.0-c0003",
        "name": "Liteyuki v0.1.0-c0003",
        "html_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/tag/v0.1.0-c0003",
        "body": "new stable release",
        "draft": false,
        "prerelease": false,
        "created_at": "2026-04-29T09:04:40Z",
        "published_at": "2026-04-29T09:11:59Z",
        "assets": [
          {
            "name": "Liteyuki_0.1.0_x64-setup.exe",
            "content_type": "application/vnd.microsoft.portable-executable",
            "size": 6935326,
            "download_count": 1,
            "browser_download_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/download/v0.1.0-c0003/Liteyuki_0.1.0_x64-setup.exe",
            "created_at": "2026-04-29T09:17:34Z",
            "updated_at": "2026-04-29T09:17:35Z"
          },
          {
            "name": "Liteyuki_0.1.0_x64_en-US.msi",
            "content_type": "application/x-msi",
            "size": 10301440,
            "download_count": 1,
            "browser_download_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/download/v0.1.0-c0003/Liteyuki_0.1.0_x64_en-US.msi",
            "created_at": "2026-04-29T09:17:32Z",
            "updated_at": "2026-04-29T09:17:33Z"
          },
          {
            "name": "Liteyuki_0.1.0_aarch64.dmg",
            "content_type": "application/x-apple-diskimage",
            "size": 10462257,
            "download_count": 0,
            "browser_download_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/download/v0.1.0-c0003/Liteyuki_0.1.0_aarch64.dmg",
            "created_at": "2026-04-29T09:12:00Z",
            "updated_at": "2026-04-29T09:12:01Z"
          },
          {
            "name": "Liteyuki_x86_64.AppImage",
            "content_type": "application/octet-stream",
            "size": 12462257,
            "download_count": 0,
            "browser_download_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/download/v0.1.0-c0003/Liteyuki_x86_64.AppImage",
            "created_at": "2026-04-29T09:12:00Z",
            "updated_at": "2026-04-29T09:12:01Z"
          }
        ]
      },
      {
        "tag_name": "v0.1.0-c0004",
        "name": "Liteyuki v0.1.0-c0004-rc1",
        "html_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/tag/v0.1.0-c0004",
        "body": "prerelease build",
        "draft": false,
        "prerelease": true,
        "created_at": "2026-04-30T09:04:40Z",
        "published_at": "2026-04-30T09:11:59Z",
        "assets": [
          {
            "name": "Liteyuki_0.1.0_x64-setup.exe",
            "content_type": "application/vnd.microsoft.portable-executable",
            "size": 6935326,
            "download_count": 1,
            "browser_download_url": "https://github.com/Nanaloveyuki/RsLiteyukiBot/releases/download/v0.1.0-c0004/Liteyuki_0.1.0_x64-setup.exe",
            "created_at": "2026-04-30T09:17:34Z",
            "updated_at": "2026-04-30T09:17:35Z"
          }
        ]
      }
    ])
}

fn expected_asset_name() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "Liteyuki_0.1.0_x64-setup.exe",
        ("macos", "aarch64") => "Liteyuki_0.1.0_aarch64.dmg",
        ("linux", "x86_64") => "Liteyuki_x86_64.AppImage",
        _ => "",
    }
}

async fn spawn_web_host() -> (String, String, tokio::task::JoinHandle<std::io::Result<()>>) {
    let snapshot_provider = Arc::new(AppHostSnapshot::default);
    let config = WebHostConfig {
        bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        browser_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        dev_frontend: None,
    };
    let (service, listener) = WebHostService::bind(config, snapshot_provider, build_default_web_host_assets())
        .expect("web host should bind");
    let token = service.local_token();
    let base_url = format!("http://{}", service.bind_addr());
    let task = tokio::spawn(service.serve(listener));
    (base_url, token, task)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn latest_release_endpoint_returns_sorted_stable_release() {
    let (api_base, fake_api_task) = spawn_fake_github_api().await;
    let _env = ReleaseApiTestEnv::new(api_base.as_str());
    let (base_url, token, web_host_task) = spawn_web_host().await;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{base_url}/api/base/GetLatestRelease"))
        .bearer_auth(token)
        .send()
        .await
        .expect("latest release request should succeed");
    let payload: Value = response
        .json()
        .await
        .expect("latest release response should be valid json");

    assert_eq!(payload["code"], 0);
    assert_eq!(payload["data"]["latest"]["tag"], "v0.1.0-c0003");
    assert_eq!(payload["data"]["latest"]["type"], "release");

    let expected_asset = expected_asset_name();
    if expected_asset.is_empty() {
        assert!(payload["data"]["latest"]["recommendedAsset"].is_null());
    } else {
        assert_eq!(
            payload["data"]["latest"]["recommendedAsset"]["name"],
            expected_asset
        );
    }

    web_host_task.abort();
    fake_api_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn release_list_endpoint_filters_invalid_tags_and_keeps_mirror_links() {
    let (api_base, fake_api_task) = spawn_fake_github_api().await;
    let _env = ReleaseApiTestEnv::new(api_base.as_str());
    let (base_url, token, web_host_task) = spawn_web_host().await;

    let mirror = "https://ghproxy.example";
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "{base_url}/api/base/getAllReleases?type=all&page=1&pageSize=10&mirror={mirror}"
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("release list request should succeed");
    let payload: Value = response
        .json()
        .await
        .expect("release list response should be valid json");

    assert_eq!(payload["code"], 0);
    assert_eq!(payload["data"]["pagination"]["total"], 3);
    assert_eq!(payload["data"]["versions"][0]["tag"], "v0.1.0-c0004");
    assert_eq!(payload["data"]["versions"][1]["tag"], "v0.1.0-c0003");
    assert_eq!(payload["data"]["versions"][2]["tag"], "v0.1.0-c0001");
    assert!(
        payload["data"]["versions"]
            .as_array()
            .expect("versions should be an array")
            .iter()
            .all(|item| item["tag"] != "nightly-build")
    );
    assert_eq!(payload["data"]["mirror"], mirror);
    assert!(
        payload["data"]["versions"][1]["mirrorHtmlUrl"]
            .as_str()
            .expect("mirror html url should exist")
            .starts_with(mirror)
    );

    web_host_task.abort();
    fake_api_task.abort();
}
