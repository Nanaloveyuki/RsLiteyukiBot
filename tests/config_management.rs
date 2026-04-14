use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use liteyukibot_core::LogLevel;
use liteyukibot_core::bootstrap::{ConfigManager, RuntimeSettings};

fn env_lock() -> &'static Mutex<()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: test code serializes env mutation with a global mutex.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: test code serializes env mutation with a global mutex.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => {
                // SAFETY: test code serializes env mutation with a global mutex.
                unsafe {
                    std::env::set_var(self.key, value);
                }
            }
            None => {
                // SAFETY: test code serializes env mutation with a global mutex.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }
}

struct TempFileGuard {
    path: PathBuf,
}

impl TempFileGuard {
    fn create(extension: &str, content: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let file_name = format!("liteyuki-config-test-{nanos}.{extension}");
        let path = std::env::temp_dir().join(file_name);
        std::fs::write(&path, content).expect("temp config file should be created");
        Self { path }
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn runtime_settings_from_manager_uses_defaults() {
    let manager = ConfigManager::new();
    let settings = RuntimeSettings::from_manager(&manager);

    assert_eq!(settings.runtime_config.worker_count, 4);
    assert_eq!(settings.runtime_config.ingress_queue, 1024);
    assert_eq!(settings.runtime_config.worker_queue, 256);
}

#[test]
fn runtime_settings_from_manager_reads_map_values() {
    let manager = ConfigManager::from_pairs([
        ("LY_WORKERS", "8"),
        ("LY_INGRESS_QUEUE", "2048"),
        ("LY_WORKER_QUEUE", "512"),
    ]);
    let settings = RuntimeSettings::from_manager(&manager);

    assert_eq!(settings.runtime_config.worker_count, 8);
    assert_eq!(settings.runtime_config.ingress_queue, 2048);
    assert_eq!(settings.runtime_config.worker_queue, 512);
}

#[test]
fn runtime_settings_from_manager_falls_back_on_invalid_values() {
    let manager = ConfigManager::from_pairs([
        ("LY_WORKERS", "0"),
        ("LY_INGRESS_QUEUE", "not-a-number"),
        ("LY_WORKER_QUEUE", "-1"),
    ]);
    let settings = RuntimeSettings::from_manager(&manager);

    assert_eq!(settings.runtime_config.worker_count, 4);
    assert_eq!(settings.runtime_config.ingress_queue, 1024);
    assert_eq!(settings.runtime_config.worker_queue, 256);
}

#[test]
fn config_manager_loads_yaml_file_and_maps_fields() {
    let file = TempFileGuard::create(
        "yaml",
        r#"
rust:
  runtime:
    worker_count: 9
    ingress_queue: 4096
    worker_queue: 2048
  log:
    mode: mono
    level: warn
    timezone: utc
    timestamp_format: custom
    timestamp_pattern: "%Y/%m/%d %H:%M:%S"
"#,
    );

    let manager = ConfigManager::load_from_path(&file.path).expect("yaml should parse");
    let settings = RuntimeSettings::from_manager(&manager);

    assert_eq!(settings.runtime_config.worker_count, 9);
    assert_eq!(settings.runtime_config.ingress_queue, 4096);
    assert_eq!(settings.runtime_config.worker_queue, 2048);
    assert_eq!(settings.runtime_config.logger.min_level, LogLevel::Warn);
    assert_eq!(manager.get("LY_LOG_MODE"), Some("mono"));
    assert_eq!(manager.get("LY_LOG_LEVEL"), Some("warn"));
    assert_eq!(manager.get("LY_LOG_TZ"), Some("utc"));
    assert_eq!(manager.get("LY_LOG_TS_FORMAT"), Some("custom"));
}

#[test]
fn config_manager_loads_toml_file() {
    let file = TempFileGuard::create(
        "toml",
        r#"
[runtime]
worker_count = 11
ingress_queue = 3000
worker_queue = 1500

[log]
mode = "color"
level = "error"
timezone = "local"
timestamp_format = "epoch_ms"
"#,
    );

    let manager = ConfigManager::load_from_path(&file.path).expect("toml should parse");
    let settings = RuntimeSettings::from_manager_with_spec(
        &manager,
        &liteyukibot_core::bootstrap::RuntimeSettingsSpec::default(),
    );

    assert_eq!(settings.runtime_config.worker_count, 11);
    assert_eq!(settings.runtime_config.ingress_queue, 3000);
    assert_eq!(settings.runtime_config.worker_queue, 1500);
    assert_eq!(settings.runtime_config.logger.min_level, LogLevel::Error);
    assert_eq!(manager.get("LY_LOG_LEVEL"), Some("error"));
}

#[test]
fn config_manager_with_env_overrides_memory_map() {
    let _lock = env_lock().lock().expect("env lock must be available");
    let _worker_guard = EnvVarGuard::set("LY_WORKERS", "12");

    let mut map = HashMap::new();
    map.insert("LY_WORKERS".to_string(), "3".to_string());

    let settings = RuntimeSettings::from_map_with_env(map);
    assert_eq!(settings.runtime_config.worker_count, 12);
}

#[test]
fn runtime_settings_try_load_honors_ly_config_path_and_env_override() {
    let _lock = env_lock().lock().expect("env lock must be available");
    let file = TempFileGuard::create(
        "yaml",
        r#"
rust:
  runtime:
    worker_count: 7
    ingress_queue: 700
    worker_queue: 70
"#,
    );
    let _config_path = EnvVarGuard::set("LY_CONFIG_PATH", file.path.to_str().expect("utf8 path"));
    let _workers_override = EnvVarGuard::set("LY_WORKERS", "15");

    let settings = RuntimeSettings::try_load().expect("config should load");
    assert_eq!(settings.runtime_config.worker_count, 15);
    assert_eq!(settings.runtime_config.ingress_queue, 700);
    assert_eq!(settings.runtime_config.worker_queue, 70);
}

#[test]
fn runtime_settings_from_env_keeps_existing_behavior() {
    let _lock = env_lock().lock().expect("env lock must be available");
    let _clear_config_path = EnvVarGuard::remove("LY_CONFIG_PATH");
    let _workers = EnvVarGuard::set("LY_WORKERS", "10");
    let _ingress = EnvVarGuard::set("LY_INGRESS_QUEUE", "3000");
    let _worker_queue = EnvVarGuard::set("LY_WORKER_QUEUE", "600");
    let _log_level = EnvVarGuard::set("LY_LOG_LEVEL", "warn");

    let settings = RuntimeSettings::from_env();
    assert_eq!(settings.runtime_config.worker_count, 10);
    assert_eq!(settings.runtime_config.ingress_queue, 3000);
    assert_eq!(settings.runtime_config.worker_queue, 600);
    assert_eq!(settings.runtime_config.logger.min_level, LogLevel::Warn);
}

#[test]
fn config_manager_can_optionally_load_env() {
    let _lock = env_lock().lock().expect("env lock must be available");
    let _guard = EnvVarGuard::set("LY_WORKERS", "14");
    let _cleanup_ingress = EnvVarGuard::remove("LY_INGRESS_QUEUE");

    let manager = ConfigManager::new();
    assert_eq!(manager.get("LY_WORKERS"), None);

    let with_env = manager.with_env();
    assert_eq!(with_env.get("LY_WORKERS"), Some("14"));
}
