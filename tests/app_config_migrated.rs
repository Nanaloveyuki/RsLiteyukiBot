pub use liteyukibot_core::{BotEvent, PluginManifestLoader, PluginSdk, SessionEvent, SessionScope};

#[allow(dead_code)]
#[path = "../src/app_config.rs"]
mod app_config;
#[allow(dead_code, unused_imports)]
#[path = "../src/command_registry.rs"]
mod command_registry;
#[allow(dead_code)]
#[path = "../src/config_paths.rs"]
mod config_paths;
#[allow(dead_code, unused_imports)]
#[path = "../src/i18n.rs"]
mod i18n;
#[allow(dead_code, unused_imports)]
#[path = "../src/llm/mod.rs"]
mod llm;
#[allow(dead_code, unused_imports)]
#[path = "../src/onebot_support.rs"]
mod onebot_support;
#[allow(dead_code, unused_imports)]
#[path = "../src/runtime_support.rs"]
mod runtime_support;
#[allow(dead_code, unused_imports)]
#[path = "../src/superuser.rs"]
mod superuser;
#[allow(dead_code, unused_imports)]
#[path = "../src/tui/mod.rs"]
mod tui;

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use app_config::*;
use liteyukibot_core::AdapterConfig;

fn env_lock() -> &'static Mutex<()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

fn temp_path(name: &str, ext: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.push(format!("rsliteyuki-{name}-{nanos}.{ext}"));
    path
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

struct CurrentDirGuard {
    previous: PathBuf,
}

impl CurrentDirGuard {
    fn set(path: &std::path::Path) -> Self {
        let previous = std::env::current_dir().expect("current dir should exist");
        std::env::set_current_dir(path).expect("current dir should be updated");
        Self { previous }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

#[test]
fn write_default_config_if_missing_creates_yaml_template() {
    let path = temp_path("config-create", "yaml");
    let _ = std::fs::remove_file(&path);

    write_default_config_if_missing(&path).expect("config file should be created");
    let content = std::fs::read_to_string(&path).expect("config file should be readable");
    assert!(content.contains("core:"));
    assert!(content.contains("adapters: []"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn resolve_app_config_path_prefers_user_configs_directory() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let base = temp_path("user-config-home", "dir");
    let _ = std::fs::remove_dir_all(&base);
    let config_dir = base.join(".liteyuki").join("configs");
    std::fs::create_dir_all(&config_dir).expect("config dir should be created");
    let config_path = config_dir.join("config.yaml");
    std::fs::write(&config_path, "core:\n  adapters: []\n").expect("config should be written");
    let cwd = temp_path("user-config-cwd", "dir");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("cwd should be created");

    let _userprofile = EnvVarGuard::set("USERPROFILE", base.to_str().expect("utf8 path"));
    let _home = EnvVarGuard::remove("HOME");
    let _config = EnvVarGuard::remove("LY_CONFIG_PATH");
    let _cwd = CurrentDirGuard::set(&cwd);

    assert_eq!(resolve_app_config_path(), Some(config_path.clone()));

    let _ = std::fs::remove_dir_all(base);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn ensure_default_config_files_creates_user_configs_config() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let base = temp_path("default-config-home", "dir");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("home dir should be created");
    let expected = base.join(".liteyuki").join("configs").join("config.yaml");
    let cwd = temp_path("default-config-cwd", "dir");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("cwd should be created");

    let _userprofile = EnvVarGuard::set("USERPROFILE", base.to_str().expect("utf8 path"));
    let _home = EnvVarGuard::remove("HOME");
    let _config = EnvVarGuard::remove("LY_CONFIG_PATH");
    let _cwd = CurrentDirGuard::set(&cwd);

    ensure_default_config_files().expect("default config should be created");
    assert!(
        expected.exists(),
        "expected config at {}",
        expected.display()
    );

    let _ = std::fs::remove_dir_all(base);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn ensure_default_config_files_migrates_legacy_root_config_to_user_configs() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let base = temp_path("migrate-config-home", "dir");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("home dir should be created");
    let expected = base.join(".liteyuki").join("configs").join("config.yaml");
    let cwd = temp_path("migrate-config-cwd", "dir");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("cwd should be created");
    let legacy = cwd.join("config.yaml");
    std::fs::write(&legacy, "core:\n  adapters: []\n").expect("legacy config should be written");

    let _userprofile = EnvVarGuard::set("USERPROFILE", base.to_str().expect("utf8 path"));
    let _home = EnvVarGuard::remove("HOME");
    let _config = EnvVarGuard::remove("LY_CONFIG_PATH");
    let _cwd = CurrentDirGuard::set(&cwd);

    ensure_default_config_files().expect("legacy config should be migrated");
    assert!(
        expected.exists(),
        "expected migrated config at {}",
        expected.display()
    );
    assert!(
        !legacy.exists(),
        "expected legacy config to be moved away from {}",
        legacy.display()
    );
    assert_eq!(
        std::fs::read_to_string(&expected).expect("migrated config should be readable"),
        "core:\n  adapters: []\n"
    );

    let _ = std::fs::remove_dir_all(base);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn ensure_default_config_files_replaces_default_user_config_with_legacy_root_config() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let base = temp_path("replace-config-home", "dir");
    let _ = std::fs::remove_dir_all(&base);
    let cwd = temp_path("replace-config-cwd", "dir");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("cwd should be created");

    let _userprofile = EnvVarGuard::set("USERPROFILE", base.to_str().expect("utf8 path"));
    let _home = EnvVarGuard::remove("HOME");
    let _config = EnvVarGuard::remove("LY_CONFIG_PATH");
    let _cwd = CurrentDirGuard::set(&cwd);

    ensure_default_config_files().expect("default user config should be created");
    let user_config = base.join(".liteyuki").join("configs").join("config.yaml");
    let legacy = cwd.join("config.yaml");
    std::fs::write(&legacy, "core:\n  log:\n    level: debug\n")
        .expect("legacy config should be written");

    ensure_default_config_files().expect("legacy config should replace default user config");
    assert_eq!(
        std::fs::read_to_string(&user_config).expect("user config should be readable"),
        "core:\n  log:\n    level: debug\n"
    );
    assert!(
        !legacy.exists(),
        "legacy config should be removed after replacement"
    );

    let _ = std::fs::remove_dir_all(base);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn resolve_password_config_path_defaults_to_user_configs_directory() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let base = temp_path("password-config-home", "dir");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("home dir should be created");
    let cwd = temp_path("password-config-cwd", "dir");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("cwd should be created");

    let _userprofile = EnvVarGuard::set("USERPROFILE", base.to_str().expect("utf8 path"));
    let _home = EnvVarGuard::remove("HOME");
    let _password = EnvVarGuard::remove("LY_PASSWORD_PATH");
    let _cwd = CurrentDirGuard::set(&cwd);

    assert_eq!(
        runtime_support::resolve_password_config_path(),
        base.join(".liteyuki").join("configs").join("password.yaml")
    );

    let _ = std::fs::remove_dir_all(base);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn ensure_default_llm_config_file_creates_user_configs_llm_config() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let base = temp_path("llm-config-home", "dir");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("home dir should be created");
    let expected = base
        .join(".liteyuki")
        .join("configs")
        .join("llm-config.yaml");
    let cwd = temp_path("llm-config-cwd", "dir");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("cwd should be created");

    let _userprofile = EnvVarGuard::set("USERPROFILE", base.to_str().expect("utf8 path"));
    let _home = EnvVarGuard::remove("HOME");
    let _llm = EnvVarGuard::remove("LY_LLM_CONFIG_PATH");
    let _cwd = CurrentDirGuard::set(&cwd);

    runtime_support::ensure_default_llm_config_file().expect("llm config should be created");
    assert!(
        expected.exists(),
        "expected llm config at {}",
        expected.display()
    );

    let _ = std::fs::remove_dir_all(base);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn ensure_default_llm_config_file_migrates_legacy_root_llm_config_to_user_configs() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let base = temp_path("migrate-llm-home", "dir");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("home dir should be created");
    let expected = base
        .join(".liteyuki")
        .join("configs")
        .join("llm-config.yaml");
    let cwd = temp_path("migrate-llm-cwd", "dir");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("cwd should be created");
    let legacy = cwd.join("llm-config.yaml");
    std::fs::write(
        &legacy,
        "llm:\n  enabled: true\n  provider: openai\n  model: gpt-5-mini\n  api_keys:\n    - sk-test\n",
    )
    .expect("legacy llm config should be written");

    let _userprofile = EnvVarGuard::set("USERPROFILE", base.to_str().expect("utf8 path"));
    let _home = EnvVarGuard::remove("HOME");
    let _llm = EnvVarGuard::remove("LY_LLM_CONFIG_PATH");
    let _cwd = CurrentDirGuard::set(&cwd);

    runtime_support::ensure_default_llm_config_file()
        .expect("legacy llm config should be migrated");
    assert!(
        expected.exists(),
        "expected migrated llm config at {}",
        expected.display()
    );
    assert!(
        !legacy.exists(),
        "expected legacy llm config to be moved away from {}",
        legacy.display()
    );
    let migrated =
        std::fs::read_to_string(&expected).expect("migrated llm config should be readable");
    assert!(migrated.contains("enabled: true"));
    assert!(migrated.contains("gpt-5-mini"));

    let _ = std::fs::remove_dir_all(base);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn ensure_default_llm_config_file_replaces_default_user_llm_config_with_legacy_root_config() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let base = temp_path("replace-llm-home", "dir");
    let _ = std::fs::remove_dir_all(&base);
    let config_dir = base.join(".liteyuki").join("configs");
    std::fs::create_dir_all(&config_dir).expect("config dir should be created");
    let user_config = config_dir.join("llm-config.yaml");
    std::fs::write(
        &user_config,
        "llm:\n  enabled: false\n  provider: openai\n  base_url: https://tokenflux.dev/v1\n  model: gpt-4.1-mini\n  timeout_seconds: 20\n  command_prefix: /ask\n  api_keys: []\n",
    )
    .expect("default user llm config should be written");
    let cwd = temp_path("replace-llm-cwd", "dir");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("cwd should be created");
    let legacy = cwd.join("llm-config.yaml");
    std::fs::write(
        &legacy,
        "llm:\n  enabled: true\n  provider: openai\n  model: gpt-5.2\n  api_keys:\n    - sk-test\n",
    )
    .expect("legacy llm config should be written");

    let _userprofile = EnvVarGuard::set("USERPROFILE", base.to_str().expect("utf8 path"));
    let _home = EnvVarGuard::remove("HOME");
    let _llm = EnvVarGuard::remove("LY_LLM_CONFIG_PATH");
    let _cwd = CurrentDirGuard::set(&cwd);

    runtime_support::ensure_default_llm_config_file()
        .expect("legacy llm config should replace default user config");
    let content =
        std::fs::read_to_string(&user_config).expect("user llm config should be readable");
    assert!(content.contains("enabled: true"));
    assert!(content.contains("gpt-5.2"));
    assert!(
        !legacy.exists(),
        "legacy llm config should be removed after replacement"
    );

    let _ = std::fs::remove_dir_all(base);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn validate_app_config_reports_invalid_values() {
    let mut duplicate = AdapterConfig::default();
    duplicate.id = "dup".to_string();

    let mut invalid = AdapterConfig::default();
    invalid.id = "dup".to_string();
    invalid.endpoint.url = "".to_string();

    let doc = AppConfigDoc {
        rust: Some(AppRustSection {
            runtime: None,
            log: None,
            adapters: Some(vec![duplicate, invalid]),
            tui: Some(TuiConfigSection {
                resume: Some(TuiResumeSection {
                    store_path: Some("   ".to_string()),
                    max_sessions: Some(0),
                    max_size_mib: Some(0),
                }),
            }),
            i18n: None,
            commands: None,
            plugins: None,
        }),
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let warnings = validate_app_config(&doc);
    assert!(warnings.iter().any(|w| w.contains("duplicated adapter id")));
    assert!(warnings.iter().any(|w| w.contains("invalid adapter")));
    assert!(warnings.iter().any(|w| w.contains("store_path")));
    assert!(warnings.iter().any(|w| w.contains("max_sessions")));
    assert!(warnings.iter().any(|w| w.contains("max_size_mib")));
}

#[test]
fn runtime_reload_warnings_detect_low_level_runtime_fields() {
    let doc = AppConfigDoc {
        rust: Some(AppRustSection {
            runtime: Some(RuntimeConfigSection {
                worker_count: Some(8),
                ingress_queue: None,
                worker_queue: None,
            }),
            log: None,
            adapters: None,
            tui: None,
            i18n: None,
            commands: None,
            plugins: None,
        }),
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let current = ReloadWarningState::from_doc(&doc);
    let warnings = runtime_reload_warnings(None, &current);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("hot switching may cause unpredictable behavior"))
    );
}

#[test]
fn runtime_reload_warnings_skip_when_sensitive_fields_unchanged() {
    let doc = AppConfigDoc {
        rust: Some(AppRustSection {
            runtime: Some(RuntimeConfigSection {
                worker_count: Some(8),
                ingress_queue: Some(1024),
                worker_queue: Some(256),
            }),
            log: Some(LogConfigSection {
                mode: Some("color".to_string()),
                level: Some("info".to_string()),
                timezone: Some("local".to_string()),
                timestamp_format: Some("custom".to_string()),
                timestamp_pattern: Some("%Y-%m-%d %H:%M:%S".to_string()),
            }),
            adapters: None,
            tui: None,
            i18n: None,
            commands: None,
            plugins: None,
        }),
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let state = ReloadWarningState::from_doc(&doc);
    let warnings = runtime_reload_warnings(Some(&state), &state);
    assert!(warnings.is_empty());
}

#[test]
fn connect_websocket_both_mode_generates_forward_and_reverse_adapters() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: Some(ConnectConfigSection {
            websocket: Some(WebSocketConnectSection {
                enabled: Some(true),
                mode: Some("both".to_string()),
                url: Some("ws://127.0.0.1:3000/ws".to_string()),
                urls: None,
                host: Some("0.0.0.0".to_string()),
                port: Some(8080),
                path: Some("/ws".to_string()),
                headers: None,
                token: None,
                timeout_seconds: Some(30),
                queue_capacity: Some(256),
                max_payload_size: Some(1024 * 1024),
                max_connections: Some(100),
                inbound_topic: None,
                outbound_topic: None,
                forward: None,
                reverse: None,
            }),
            tcp_http: None,
            sse: None,
        }),
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let adapters = load_adapter_configs(&doc).expect("connect adapters should parse");
    assert!(
        adapters
            .iter()
            .any(|adapter| adapter.id == "connect-ws-forward")
    );
    assert!(
        adapters
            .iter()
            .any(|adapter| adapter.id == "connect-ws-reverse")
    );
}

#[test]
fn connect_websocket_port_without_mode_defaults_to_reverse() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: Some(ConnectConfigSection {
            websocket: Some(WebSocketConnectSection {
                enabled: Some(true),
                mode: None,
                url: None,
                urls: None,
                host: Some("0.0.0.0".to_string()),
                port: Some(8090),
                path: Some("/ws".to_string()),
                headers: None,
                token: None,
                timeout_seconds: Some(30),
                queue_capacity: None,
                max_payload_size: Some(1024 * 1024),
                max_connections: Some(100),
                inbound_topic: None,
                outbound_topic: None,
                forward: None,
                reverse: None,
            }),
            tcp_http: None,
            sse: None,
        }),
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let adapters = load_adapter_configs(&doc).expect("connect adapters should parse");
    assert_eq!(adapters.len(), 1);
    assert_eq!(adapters[0].id, "connect-ws-reverse");
    assert_eq!(adapters[0].max_payload_size, Some(1024 * 1024));
    assert_eq!(adapters[0].max_connections, Some(100));
}

#[test]
fn connect_websocket_urls_expand_to_multiple_adapters() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: Some(ConnectConfigSection {
            websocket: Some(WebSocketConnectSection {
                enabled: Some(true),
                mode: Some("forward".to_string()),
                url: None,
                urls: Some(vec![
                    "ws://127.0.0.1:3100/ws".to_string(),
                    "ws://127.0.0.1:3200/ws".to_string(),
                ]),
                host: None,
                port: None,
                path: None,
                headers: None,
                token: None,
                timeout_seconds: Some(30),
                queue_capacity: Some(128),
                max_payload_size: Some(1024 * 1024),
                max_connections: Some(100),
                inbound_topic: None,
                outbound_topic: None,
                forward: None,
                reverse: None,
            }),
            tcp_http: None,
            sse: None,
        }),
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let adapters = load_adapter_configs(&doc).expect("connect adapters should parse");
    assert_eq!(adapters.len(), 2);
    assert_eq!(adapters[0].id, "connect-ws-forward-1");
    assert_eq!(adapters[1].id, "connect-ws-forward-2");
    assert_eq!(adapters[0].endpoint.url, "ws://127.0.0.1:3100/ws");
    assert_eq!(adapters[1].endpoint.url, "ws://127.0.0.1:3200/ws");
}

#[test]
fn connect_http_urls_expand_to_multiple_adapters() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: Some(ConnectConfigSection {
            websocket: None,
            tcp_http: Some(HttpConnectSection {
                enabled: Some(true),
                url: None,
                urls: Some(vec![
                    "http://127.0.0.1:8081/".to_string(),
                    "http://127.0.0.1:8083/".to_string(),
                ]),
                host: None,
                port: None,
                path: None,
                headers: None,
                token: None,
                timeout_seconds: Some(30),
                queue_capacity: Some(64),
                max_payload_size: Some(2048),
                max_connections: Some(8),
                inbound_topic: None,
                outbound_topic: None,
            }),
            sse: None,
        }),
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let adapters = load_adapter_configs(&doc).expect("connect adapters should parse");
    assert_eq!(adapters.len(), 2);
    assert_eq!(adapters[0].id, "connect-http-1");
    assert_eq!(adapters[1].id, "connect-http-2");
    assert_eq!(adapters[0].endpoint.url, "http://127.0.0.1:8081/");
    assert_eq!(adapters[1].endpoint.url, "http://127.0.0.1:8083/");
}

#[test]
fn load_app_config_accepts_core_root() {
    let path = temp_path("core-root", "yaml");
    let content = "core:\n  runtime:\n    worker_count: 6\n";
    std::fs::write(&path, content).expect("should write temp config");

    let doc = load_app_config_from_path(&path).expect("should parse with core root");
    let worker_count = doc
        .rust
        .as_ref()
        .and_then(|section| section.runtime.as_ref())
        .and_then(|runtime| runtime.worker_count);
    assert_eq!(worker_count, Some(6));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn resolve_help_whitelist_accepts_numeric_and_prefixed_entries() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: Some(OnebotV11ConfigSection {
            whitelist: vec![
                OnebotWhitelistEntry::UInt(3541766758),
                OnebotWhitelistEntry::Text("group:699493240".to_string()),
                OnebotWhitelistEntry::Text("   ".to_string()),
            ],
        }),
    };

    let whitelist = resolve_help_whitelist(&doc);
    assert!(whitelist.contains("3541766758"));
    assert!(whitelist.contains("group:699493240"));
    assert!(!whitelist.contains(""));
}

#[test]
fn validate_app_config_warns_empty_onebot_whitelist_entry() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: Some(OnebotV11ConfigSection {
            whitelist: vec![OnebotWhitelistEntry::Text("  ".to_string())],
        }),
    };
    let warnings = validate_app_config(&doc);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("onebot-v11.whitelist"))
    );
}

#[test]
fn resolve_disabled_scope_commands_normalizes_and_deduplicates_entries() {
    let doc = AppConfigDoc {
        rust: Some(AppRustSection {
            runtime: None,
            log: None,
            adapters: None,
            tui: None,
            i18n: None,
            commands: Some(CommandConfigSection {
                disabled: vec![
                    " onebot11 liteecho ".to_string(),
                    "adapter:onebot_v11 /LiteEcho".to_string(),
                    "tui help".to_string(),
                    " ".to_string(),
                ],
            }),
            plugins: None,
        }),
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let disabled = resolve_disabled_scope_commands(&doc);
    assert_eq!(
        disabled,
        vec![
            "adapter:onebot11 /liteecho".to_string(),
            "tui /help".to_string(),
        ]
    );
}

#[test]
fn validate_app_config_warns_invalid_disabled_command_entry() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: Some(CommandConfigSection {
            disabled: vec!["adapter:discord ping".to_string()],
        }),
        plugins: None,
        onebot_v11: None,
    };

    let warnings = validate_app_config(&doc);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("commands.disabled entries should use"))
    );
}

#[test]
fn resolve_disabled_plugins_normalizes_and_deduplicates_entries() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: Some(PluginConfigSection {
            disabled: vec![
                " builtin-liteecho ".to_string(),
                "BUILTIN-LITEECHO".to_string(),
                "demo-plugin".to_string(),
                " ".to_string(),
            ],
        }),
        onebot_v11: None,
    };

    let disabled = resolve_disabled_plugins(&doc);
    assert_eq!(
        disabled,
        vec!["builtin-liteecho".to_string(), "demo-plugin".to_string(),]
    );
}

#[test]
fn validate_app_config_warns_invalid_disabled_plugin_entry() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        commands: None,
        plugins: Some(PluginConfigSection {
            disabled: vec!["   ".to_string()],
        }),
        onebot_v11: None,
    };

    let warnings = validate_app_config(&doc);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("plugins.disabled entries should use"))
    );
}

#[test]
fn resolve_llm_config_reads_values_from_config() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: Some(LlmConfigSection {
            enabled: Some(true),
            stream: Some(true),
            provider: Some("openai".to_string()),
            base_url: Some("https://api.openai.com/".to_string()),
            provider_urls: Some(vec![
                "https://api.openai.com/".to_string(),
                "https://tokenflux.dev/v1".to_string(),
            ]),
            api_keys: Some(vec!["sk-test".to_string(), "sk-b".to_string()]),
            api_key: Some("sk-test".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            timeout_seconds: Some(12),
            temperature: Some(0.7),
            top_p: Some(0.95),
            top_k: Some(32),
            parallel_tool_calls: Some(false),
            system_prompt: Some("system".to_string()),
            command_prefix: Some("/ask".to_string()),
        }),
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let config = resolve_llm_config(&doc);
    assert!(config.enabled);
    assert!(config.stream);
    assert_eq!(config.provider, "openai");
    assert_eq!(config.base_url, "https://api.openai.com");
    assert_eq!(config.api_keys.len(), 2);
    assert_eq!(config.api_keys.first().map(|s| s.as_str()), Some("sk-test"));
    assert_eq!(config.timeout_ms, 12_000);
    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.top_p, Some(0.95));
    assert_eq!(config.top_k, Some(32));
    assert!(!config.parallel_tool_calls);
}

#[test]
fn resolve_llm_config_falls_back_to_provider_urls_when_base_url_missing() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: Some(LlmConfigSection {
            enabled: Some(true),
            stream: None,
            provider: Some("openai".to_string()),
            base_url: None,
            provider_urls: Some(vec![
                "https://tokenflux.dev/v1/".to_string(),
                "https://api.openai.com".to_string(),
            ]),
            api_keys: Some(vec!["sk-test".to_string()]),
            api_key: None,
            model: Some("gpt-4.1-mini".to_string()),
            timeout_seconds: Some(20),
            temperature: None,
            top_p: None,
            top_k: None,
            parallel_tool_calls: None,
            system_prompt: None,
            command_prefix: Some("/ask".to_string()),
        }),
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let config = resolve_llm_config(&doc);
    assert_eq!(config.base_url, "https://tokenflux.dev/v1");
}

#[test]
fn validate_app_config_warns_when_llm_is_enabled_without_api_key() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: Some(LlmConfigSection {
            enabled: Some(true),
            stream: None,
            provider: Some("not-built-in".to_string()),
            base_url: None,
            provider_urls: None,
            api_keys: None,
            api_key: None,
            model: Some("".to_string()),
            timeout_seconds: Some(0),
            temperature: None,
            top_p: None,
            top_k: None,
            parallel_tool_calls: None,
            system_prompt: None,
            command_prefix: Some(" ".to_string()),
        }),
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let warnings = validate_app_config(&doc);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("llm.api_key/api_keys"))
    );
    assert!(warnings.iter().any(|warning| warning.contains("llm.model")));
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("not built-in yet"))
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("llm.timeout_seconds"))
    );
}

#[test]
fn validate_app_config_warns_llm_base_url_in_main_config() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: Some(LlmConfigSection {
            enabled: Some(true),
            stream: None,
            provider: Some("openai".to_string()),
            base_url: Some("https://tokenflux.dev/v1".to_string()),
            provider_urls: None,
            api_keys: Some(vec!["sk-test".to_string()]),
            api_key: None,
            model: Some("gpt-4.1-mini".to_string()),
            timeout_seconds: Some(20),
            temperature: None,
            top_p: None,
            top_k: None,
            parallel_tool_calls: None,
            system_prompt: None,
            command_prefix: Some("/ask".to_string()),
        }),
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let warnings = validate_app_config(&doc);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("move it to llm-config.yaml"))
    );
}

#[test]
fn validate_app_config_warns_invalid_llm_sampling_ranges() {
    let doc = AppConfigDoc {
        rust: None,
        runtime: None,
        log: None,
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: Some(LlmConfigSection {
            enabled: Some(false),
            stream: Some(true),
            provider: Some("openai".to_string()),
            base_url: None,
            provider_urls: None,
            api_keys: Some(vec!["sk-test".to_string()]),
            api_key: None,
            model: Some("gpt-4.1-mini".to_string()),
            timeout_seconds: Some(20),
            temperature: Some(2.5),
            top_p: Some(1.2),
            top_k: Some(0),
            parallel_tool_calls: Some(true),
            system_prompt: None,
            command_prefix: Some("/ask".to_string()),
        }),
        commands: None,
        plugins: None,
        onebot_v11: None,
    };

    let warnings = validate_app_config(&doc);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("llm.temperature"))
    );
    assert!(warnings.iter().any(|warning| warning.contains("0..=2")));
    assert!(warnings.iter().any(|warning| warning.contains("llm.top_p")));
    assert!(warnings.iter().any(|warning| warning.contains("0..=1")));
    assert!(warnings.iter().any(|warning| warning.contains("llm.top_k")));
}
