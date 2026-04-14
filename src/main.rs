use std::collections::HashSet;
use std::path::{Path, PathBuf};

use liteyukibot_core::{AdapterConfig, LiteyukiBot, LogLevel, RuntimeSettings, RuntimeTarget};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;

mod tui;

const APP_TITLE: &str = "RsLiteyukiBot";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;
const APP_CONFIG_PATHS: [&str; 6] = [
    "config.yaml",
    "rust-config.yaml",
    "rust-config.yml",
    "rust-config.toml",
    "config/rust-core.yaml",
    "config/rust-core.toml",
];

#[derive(Debug, Deserialize, Default)]
struct AppConfigDoc {
    #[serde(default)]
    rust: Option<AppRustSection>,
    #[serde(default)]
    adapters: Option<Vec<AdapterConfig>>,
    #[serde(default)]
    tui: Option<TuiConfigSection>,
}

#[derive(Debug, Deserialize, Default)]
struct AppRustSection {
    #[serde(default)]
    adapters: Option<Vec<AdapterConfig>>,
    #[serde(default)]
    tui: Option<TuiConfigSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct TuiConfigSection {
    #[serde(default)]
    resume: Option<TuiResumeSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct TuiResumeSection {
    #[serde(default)]
    store_path: Option<String>,
    #[serde(default)]
    max_sessions: Option<usize>,
    #[serde(default)]
    max_size_mib: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AdapterConfigDoc {
    adapters: Vec<AdapterConfig>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(err) = ensure_default_config_files() {
        eprintln!("failed to ensure default config files: {err}");
    }

    let settings = match RuntimeSettings::try_load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("failed to load runtime config from file/env, fallback to default: {err}");
            RuntimeSettings::default()
        }
    };
    let _ = settings.clone().install_global();
    let active_settings = RuntimeSettings::global_or_default();
    let mut runtime_config = RuntimeSettings::global_runtime_config().clone();
    runtime_config.logger.min_level = LogLevel::Error;

    let app_config = load_app_config();
    let target = resolve_runtime_target();
    let adapter_configs = load_adapter_configs(&app_config).unwrap_or_default();
    let adapter_autostart = !adapter_configs.is_empty();
    let tui_config = resolve_tui_config(&app_config);

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<tui::UiEvent>();
    let ui_tx_for_handler = ui_tx.clone();

    let mut bot = LiteyukiBot::builder(APP_TITLE, env!("CARGO_PKG_VERSION"))
        .with_target(target)
        .with_runtime_config(runtime_config)
        .with_adapter_configs(adapter_configs.clone())
        .with_adapter_autostart(adapter_autostart)
        .with_event_handler(move |event, _logger| {
            let ui_tx_for_handler = ui_tx_for_handler.clone();
            async move {
                let _ = ui_tx_for_handler.send(tui::UiEvent::RuntimeHandled {
                    id: event.id,
                    topic: event.topic.clone(),
                    payload_preview: payload_preview(&event.payload),
                });
            }
        })
        .build();

    let tx_before = ui_tx.clone();
    bot.on_before_start_sync("tui-before-start", Default::default(), move |_context| {
        let _ = tx_before.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: "runtime preparing...".to_string(),
        });
        Ok(())
    });

    let tx_after = ui_tx.clone();
    bot.on_after_start_sync("tui-after-start", Default::default(), move |_context| {
        let _ = tx_after.send(tui::UiEvent::Log {
            level: tui::UiLevel::Info,
            message: "runtime started".to_string(),
        });
        Ok(())
    });

    let tx_before_shutdown = ui_tx.clone();
    bot.on_before_process_shutdown_sync(
        "tui-before-shutdown",
        Default::default(),
        move |_context, process_name| {
            let _ = tx_before_shutdown.send(tui::UiEvent::Log {
                level: tui::UiLevel::Warn,
                message: format!("shutting down process: {}", process_name),
            });
            Ok(())
        },
    );

    bot.start().await?;

    let tui_result = tui::run(
        &mut bot,
        target,
        active_settings.describe(),
        adapter_configs,
        adapter_autostart,
        tui_config,
        &mut ui_rx,
    )
    .await;

    let shutdown_result = bot.shutdown().await;
    if let Err(err) = shutdown_result {
        eprintln!("bot shutdown failed: {err}");
    }

    tui_result?;
    Ok(())
}

fn resolve_runtime_target() -> RuntimeTarget {
    std::env::var("LY_RUNTIME_TARGET")
        .ok()
        .as_deref()
        .and_then(RuntimeTarget::parse)
        .unwrap_or(DEFAULT_RUNTIME_TARGET)
}

fn load_app_config() -> AppConfigDoc {
    let Some(path) = resolve_app_config_path() else {
        return AppConfigDoc::default();
    };

    match load_app_config_from_path(&path) {
        Ok(doc) => {
            for warning in validate_app_config(&doc) {
                eprintln!("config warning ({}): {warning}", path.display());
            }
            doc
        }
        Err(err) => {
            eprintln!("failed to load app config from {}: {err}", path.display());
            AppConfigDoc::default()
        }
    }
}

fn resolve_app_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("LY_CONFIG_PATH") {
        return Some(PathBuf::from(path));
    }

    APP_CONFIG_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn ensure_default_config_files() -> Result<(), Box<dyn std::error::Error>> {
    write_default_config_if_missing(Path::new("config.yaml"))?;

    if let Ok(path) = std::env::var("LY_CONFIG_PATH") {
        let path = PathBuf::from(path);
        write_default_config_if_missing(path.as_path())?;
    }

    Ok(())
}

fn write_default_config_if_missing(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let template = default_config_template(path);
    std::fs::write(path, template)?;
    eprintln!("created default config file: {}", path.display());
    Ok(())
}

fn default_config_template(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match ext.as_deref() {
        Some("toml") => DEFAULT_TOML_CONFIG_TEMPLATE.to_string(),
        _ => DEFAULT_YAML_CONFIG_TEMPLATE.to_string(),
    }
}

const DEFAULT_YAML_CONFIG_TEMPLATE: &str = r#"rust:
  runtime:
    worker_count: 4
    ingress_queue: 1024
    worker_queue: 256
  log:
    mode: color
    level: info
    timezone: local
    timestamp_format: custom
    timestamp_pattern: "%Y-%m-%d %H:%M:%S"
  adapters: []
  tui:
    resume:
      store_path: ./.liteyuki-tui-resumes.json
      max_sessions: 64
      max_size_mib: 16
"#;

const DEFAULT_TOML_CONFIG_TEMPLATE: &str = r#"[rust]
adapters = []

[rust.runtime]
worker_count = 4
ingress_queue = 1024
worker_queue = 256

[rust.log]
mode = "color"
level = "info"
timezone = "local"
timestamp_format = "custom"
timestamp_pattern = "%Y-%m-%d %H:%M:%S"

[rust.tui.resume]
store_path = "./.liteyuki-tui-resumes.json"
max_sessions = 64
max_size_mib = 16
"#;

fn load_app_config_from_path(path: &Path) -> Result<AppConfigDoc, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match ext.as_deref() {
        Some("yaml") | Some("yml") => Ok(serde_yaml::from_str::<AppConfigDoc>(&content)?),
        Some("toml") => Ok(toml::from_str::<AppConfigDoc>(&content)?),
        _ => Err(format!("unsupported config extension for {}", path.display()).into()),
    }
}

fn config_adapters(doc: &AppConfigDoc) -> Option<&Vec<AdapterConfig>> {
    doc.rust
        .as_ref()
        .and_then(|section| section.adapters.as_ref())
        .or(doc.adapters.as_ref())
}

fn config_tui_resume(doc: &AppConfigDoc) -> Option<&TuiResumeSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.tui.as_ref())
        .and_then(|tui| tui.resume.as_ref())
        .or(doc.tui.as_ref().and_then(|tui| tui.resume.as_ref()))
}

fn load_adapter_configs(
    app_config: &AppConfigDoc,
) -> Result<Vec<AdapterConfig>, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("LY_ADAPTERS_PATH") {
        let content = std::fs::read_to_string(PathBuf::from(path))?;
        if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(&content) {
            return Ok(sanitize_adapter_configs(doc.adapters, "LY_ADAPTERS_PATH"));
        }
        let list = serde_json::from_str::<Vec<AdapterConfig>>(&content)?;
        return Ok(sanitize_adapter_configs(list, "LY_ADAPTERS_PATH"));
    }

    if let Ok(raw) = std::env::var("LY_ADAPTERS_JSON") {
        if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(&raw) {
            return Ok(sanitize_adapter_configs(doc.adapters, "LY_ADAPTERS_JSON"));
        }
        let list = serde_json::from_str::<Vec<AdapterConfig>>(&raw)?;
        return Ok(sanitize_adapter_configs(list, "LY_ADAPTERS_JSON"));
    }

    Ok(sanitize_adapter_configs(
        config_adapters(app_config).cloned().unwrap_or_default(),
        "config",
    ))
}

fn resolve_tui_config(app_config: &AppConfigDoc) -> tui::TuiConfig {
    let mut config = tui::TuiConfig::default();

    if let Some(resume) = config_tui_resume(app_config) {
        if let Some(path) = resume.store_path.as_deref() {
            config.resume_store_path = PathBuf::from(path);
        }
        if let Some(max_sessions) = resume.max_sessions
            && max_sessions > 0
        {
            config.resume_max_sessions = max_sessions;
        }
        if let Some(max_size_mib) = resume.max_size_mib
            && max_size_mib > 0
        {
            config.resume_max_size_mib = max_size_mib;
        }
    }

    if let Ok(path) = std::env::var("LY_RESUME_STORE_PATH") {
        config.resume_store_path = PathBuf::from(path);
    }
    if let Ok(raw) = std::env::var("LY_TUI_RESUME_MAX_SESSIONS")
        && let Ok(value) = raw.trim().parse::<usize>()
        && value > 0
    {
        config.resume_max_sessions = value;
    }
    if let Ok(raw) = std::env::var("LY_TUI_RESUME_MAX_SIZE_MIB")
        && let Ok(value) = raw.trim().parse::<u64>()
        && value > 0
    {
        config.resume_max_size_mib = value;
    }

    config
}

fn payload_preview(payload: &Value) -> String {
    let raw = payload.to_string();
    const MAX: usize = 96;
    let mut iter = raw.chars();
    let preview: String = iter.by_ref().take(MAX).collect();
    if iter.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn sanitize_adapter_configs(configs: Vec<AdapterConfig>, source: &str) -> Vec<AdapterConfig> {
    let mut sanitized = Vec::new();
    let mut seen = HashSet::new();
    for config in configs {
        if let Err(err) = config.validate() {
            eprintln!(
                "config warning ({}): skip invalid adapter '{}': {}",
                source, config.id, err
            );
            continue;
        }
        if !seen.insert(config.id.clone()) {
            eprintln!(
                "config warning ({}): skip duplicated adapter id '{}'",
                source, config.id
            );
            continue;
        }
        sanitized.push(config);
    }
    sanitized
}

fn validate_app_config(doc: &AppConfigDoc) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(adapters) = config_adapters(doc) {
        let mut seen = HashSet::new();
        for adapter in adapters {
            if let Err(err) = adapter.validate() {
                warnings.push(format!("invalid adapter '{}': {}", adapter.id, err));
            }
            if !seen.insert(adapter.id.clone()) {
                warnings.push(format!("duplicated adapter id '{}'", adapter.id));
            }
        }
    }

    if let Some(resume) = config_tui_resume(doc) {
        if let Some(path) = resume.store_path.as_deref()
            && path.trim().is_empty()
        {
            warnings.push("tui.resume.store_path should not be empty".to_string());
        }
        if let Some(max_sessions) = resume.max_sessions
            && max_sessions == 0
        {
            warnings.push("tui.resume.max_sessions should be > 0".to_string());
        }
        if let Some(max_size_mib) = resume.max_size_mib
            && max_size_mib == 0
        {
            warnings.push("tui.resume.max_size_mib should be > 0".to_string());
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str, ext: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!("rsliteyuki-{name}-{nanos}.{ext}"));
        path
    }

    #[test]
    fn write_default_config_if_missing_creates_yaml_template() {
        let path = temp_path("config-create", "yaml");
        let _ = std::fs::remove_file(&path);

        write_default_config_if_missing(&path).expect("config file should be created");
        let content = std::fs::read_to_string(&path).expect("config file should be readable");
        assert!(content.contains("rust:"));
        assert!(content.contains("adapters: []"));

        let _ = std::fs::remove_file(&path);
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
                adapters: Some(vec![duplicate, invalid]),
                tui: Some(TuiConfigSection {
                    resume: Some(TuiResumeSection {
                        store_path: Some("   ".to_string()),
                        max_sessions: Some(0),
                        max_size_mib: Some(0),
                    }),
                }),
            }),
            adapters: None,
            tui: None,
        };

        let warnings = validate_app_config(&doc);
        assert!(warnings.iter().any(|w| w.contains("duplicated adapter id")));
        assert!(warnings.iter().any(|w| w.contains("invalid adapter")));
        assert!(warnings.iter().any(|w| w.contains("store_path")));
        assert!(warnings.iter().any(|w| w.contains("max_sessions")));
        assert!(warnings.iter().any(|w| w.contains("max_size_mib")));
    }
}
