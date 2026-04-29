use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use liteyukibot_core::{persist_desktop_close_to_tray_preference, resolve_desktop_close_behavior};

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
        unsafe {
            std::env::set_var(key, value);
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

struct TempFileGuard {
    path: PathBuf,
}

impl TempFileGuard {
    fn create(extension: &str, content: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("liteyuki-desktop-close-{nanos}.{extension}"));
        std::fs::write(&path, content).expect("temp config should be written");
        Self { path }
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn desktop_close_behavior_defaults_to_prompted_background_mode_when_unconfigured() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let config = TempFileGuard::create("yaml", "core:\n  adapters: []\n");
    let _config_path = EnvVarGuard::set("LY_CONFIG_PATH", config.path.to_str().expect("utf8 path"));

    let behavior = resolve_desktop_close_behavior();

    assert!(behavior.close_to_tray);
    assert!(!behavior.configured);
}

#[test]
fn persist_desktop_close_behavior_updates_yaml_without_rewriting_other_sections() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let config = TempFileGuard::create(
        "yaml",
        "core:\n  adapters: []\n\ndesktop:\n  close_to_tray: true\n",
    );
    let _config_path = EnvVarGuard::set("LY_CONFIG_PATH", config.path.to_str().expect("utf8 path"));

    let behavior =
        persist_desktop_close_to_tray_preference(false).expect("desktop preference should persist");
    let content = std::fs::read_to_string(&config.path).expect("config should be readable");

    assert!(!behavior.close_to_tray);
    assert!(behavior.configured);
    assert!(content.contains("desktop:\n  close_to_tray: false\n"));
    assert!(content.contains("core:\n  adapters: []"));
}

#[test]
fn persist_desktop_close_behavior_inserts_toml_section_when_missing() {
    let _lock = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let config = TempFileGuard::create("toml", "[core]\nadapters = []\n");
    let _config_path = EnvVarGuard::set("LY_CONFIG_PATH", config.path.to_str().expect("utf8 path"));

    persist_desktop_close_to_tray_preference(true).expect("desktop preference should persist");
    let content = std::fs::read_to_string(&config.path).expect("config should be readable");
    let behavior = resolve_desktop_close_behavior();

    assert!(behavior.close_to_tray);
    assert!(behavior.configured);
    assert!(content.contains("[desktop]"));
    assert!(content.contains("close_to_tray = true"));
    assert!(content.contains("[core]"));
}
