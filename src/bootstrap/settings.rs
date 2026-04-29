use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::core::BotRuntimeConfig;
use crate::observability::{LogLevel, LogMode, LoggerConfig, TimeZone, TimestampFormat};
use crate::utils::config_path::resolve_existing_app_config_path;
use crate::utils::runtime_settings::{
    LOG_LEVEL_KEY, LOG_MODE_KEY, LOG_TIMESTAMP_FORMAT_KEY, LOG_TIMESTAMP_PATTERN_KEY,
    LOG_TIMEZONE_KEY, RUNTIME_INGRESS_QUEUE_KEY, RUNTIME_WORKER_COUNT_KEY,
    RUNTIME_WORKER_QUEUE_KEY, runtime_setting_value_pairs,
};

static GLOBAL_SETTINGS: OnceLock<RuntimeSettings> = OnceLock::new();

#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseYaml {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    ParseToml {
        path: PathBuf,
        source: toml::de::Error,
    },
    UnsupportedFormat {
        path: PathBuf,
    },
    GlobalAlreadyInitialized,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read config file {}: {source}", path.display())
            }
            Self::ParseYaml { path, source } => {
                write!(
                    f,
                    "failed to parse yaml config {}: {source}",
                    path.display()
                )
            }
            Self::ParseToml { path, source } => {
                write!(
                    f,
                    "failed to parse toml config {}: {source}",
                    path.display()
                )
            }
            Self::UnsupportedFormat { path } => {
                write!(f, "unsupported config format: {}", path.display())
            }
            Self::GlobalAlreadyInitialized => {
                f.write_str("global runtime settings already initialized")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSetting<T> {
    pub key: &'static str,
    pub default: T,
}

impl<T> ConfigSetting<T> {
    pub const fn new(key: &'static str, default: T) -> Self {
        Self { key, default }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSettingsSpec {
    pub worker_count: ConfigSetting<usize>,
    pub ingress_queue: ConfigSetting<usize>,
    pub worker_queue: ConfigSetting<usize>,
}

impl Default for RuntimeSettingsSpec {
    fn default() -> Self {
        Self {
            worker_count: ConfigSetting::new(RUNTIME_WORKER_COUNT_KEY, 4),
            ingress_queue: ConfigSetting::new(RUNTIME_INGRESS_QUEUE_KEY, 1024),
            worker_queue: ConfigSetting::new(RUNTIME_WORKER_QUEUE_KEY, 256),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConfigManager {
    values: HashMap<String, String>,
}

impl ConfigManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_map(values: HashMap<String, String>) -> Self {
        Self { values }
    }

    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let values = pairs
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        Self { values }
    }

    pub fn with_env(mut self) -> Self {
        for (key, value) in std::env::vars() {
            self.values.insert(key, value);
        }
        self
    }

    pub fn load_default() -> Result<Self, ConfigError> {
        if let Ok(config_path) = std::env::var("LY_CONFIG_PATH") {
            return Self::load_from_path(config_path);
        }

        let mut manager = Self::new();
        if let Some(path) = resolve_existing_app_config_path() {
            manager.merge_file(&path)?;
        }
        Ok(manager)
    }

    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let mut manager = Self::new();
        manager.merge_file(path)?;
        Ok(manager)
    }

    pub fn with_file<P: AsRef<Path>>(mut self, path: P) -> Result<Self, ConfigError> {
        self.merge_file(path)?;
        Ok(self)
    }

    pub fn insert<K, V>(&mut self, key: K, value: V)
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.values.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn get_non_zero_usize(&self, key: &str) -> Option<usize> {
        self.get(key)
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
    }

    fn merge_file<P: AsRef<Path>>(&mut self, path: P) -> Result<(), ConfigError> {
        let path_ref = path.as_ref();
        let content = std::fs::read_to_string(path_ref).map_err(|source| ConfigError::Io {
            path: path_ref.to_path_buf(),
            source,
        })?;

        let ext = path_ref
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());

        let raw = match ext.as_deref() {
            Some("yaml") | Some("yml") => {
                serde_yaml::from_str::<RawConfig>(&content).map_err(|source| {
                    ConfigError::ParseYaml {
                        path: path_ref.to_path_buf(),
                        source,
                    }
                })?
            }
            Some("toml") => {
                toml::from_str::<RawConfig>(&content).map_err(|source| ConfigError::ParseToml {
                    path: path_ref.to_path_buf(),
                    source,
                })?
            }
            _ => {
                return Err(ConfigError::UnsupportedFormat {
                    path: path_ref.to_path_buf(),
                });
            }
        };

        self.apply_raw_config(raw);
        Ok(())
    }

    fn apply_raw_config(&mut self, raw: RawConfig) {
        let (runtime, log) = match raw.rust {
            Some(rust_section) => (
                rust_section.runtime.or(raw.runtime),
                rust_section.log.or(raw.log),
            ),
            None => (raw.runtime, raw.log),
        };
        self.apply_runtime_setting_pairs(runtime_setting_value_pairs(
            runtime.as_ref().and_then(|section| section.worker_count),
            runtime.as_ref().and_then(|section| section.ingress_queue),
            runtime.as_ref().and_then(|section| section.worker_queue),
            log.as_ref().and_then(|section| section.mode.as_deref()),
            log.as_ref().and_then(|section| section.level.as_deref()),
            log.as_ref().and_then(|section| section.timezone.as_deref()),
            log.as_ref()
                .and_then(|section| section.timestamp_format.as_deref()),
            log.as_ref()
                .and_then(|section| section.timestamp_pattern.as_deref()),
        ));
    }

    fn apply_runtime_setting_pairs(&mut self, pairs: Vec<(&'static str, String)>) {
        for (key, value) in pairs {
            self.insert(key, value);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeSettings {
    pub runtime_config: BotRuntimeConfig,
}

impl RuntimeSettings {
    pub fn try_load() -> Result<Self, ConfigError> {
        let manager = ConfigManager::load_default()?.with_env();
        let spec = RuntimeSettingsSpec::default();
        Ok(Self::build(
            &manager,
            &spec,
            logger_config_from_manager(&manager),
        ))
    }

    pub fn from_env() -> Self {
        Self::try_load().unwrap_or_default()
    }

    pub fn from_map(values: HashMap<String, String>) -> Self {
        let manager = ConfigManager::from_map(values);
        Self::from_manager(&manager)
    }

    pub fn from_map_with_env(values: HashMap<String, String>) -> Self {
        let manager = ConfigManager::from_map(values).with_env();
        let spec = RuntimeSettingsSpec::default();
        Self::build(&manager, &spec, logger_config_from_manager(&manager))
    }

    pub fn from_manager(manager: &ConfigManager) -> Self {
        Self::from_manager_with_spec(manager, &RuntimeSettingsSpec::default())
    }

    pub fn from_manager_with_spec(manager: &ConfigManager, spec: &RuntimeSettingsSpec) -> Self {
        Self::build(manager, spec, logger_config_from_manager(manager))
    }

    fn build(manager: &ConfigManager, spec: &RuntimeSettingsSpec, logger: LoggerConfig) -> Self {
        let worker_count = resolve_non_zero_usize(manager, &spec.worker_count);
        let ingress_queue = resolve_non_zero_usize(manager, &spec.ingress_queue);
        let worker_queue = resolve_non_zero_usize(manager, &spec.worker_queue);

        let runtime_config = BotRuntimeConfig {
            worker_count,
            ingress_queue,
            worker_queue,
            logger,
        };

        Self { runtime_config }
    }

    pub fn install_global(self) -> Result<&'static Self, ConfigError> {
        GLOBAL_SETTINGS
            .set(self)
            .map_err(|_| ConfigError::GlobalAlreadyInitialized)?;
        Ok(GLOBAL_SETTINGS
            .get()
            .expect("global settings must exist after successful initialization"))
    }

    pub fn global() -> Option<&'static Self> {
        GLOBAL_SETTINGS.get()
    }

    pub fn global_or_default() -> &'static Self {
        GLOBAL_SETTINGS.get_or_init(Self::default)
    }

    pub fn global_runtime_config() -> &'static BotRuntimeConfig {
        &Self::global_or_default().runtime_config
    }

    pub fn describe_runtime_config(runtime_config: &BotRuntimeConfig) -> String {
        format!(
            "workers={}, ingress_queue={}, worker_queue={}, log_mode={}, log_level={}, log_tz={}, log_ts={}",
            runtime_config.worker_count,
            runtime_config.ingress_queue,
            runtime_config.worker_queue,
            runtime_config.logger.mode,
            runtime_config.logger.min_level,
            runtime_config.logger.timezone,
            runtime_config.logger.timestamp_format
        )
    }

    pub fn describe(&self) -> String {
        Self::describe_runtime_config(&self.runtime_config)
    }
}

fn resolve_non_zero_usize(manager: &ConfigManager, setting: &ConfigSetting<usize>) -> usize {
    manager
        .get_non_zero_usize(setting.key)
        .unwrap_or(setting.default)
}

fn logger_config_from_manager(manager: &ConfigManager) -> LoggerConfig {
    let mut logger = LoggerConfig::default();

    if let Some(raw) = manager.get(LOG_MODE_KEY)
        && let Some(mode) = LogMode::parse(raw)
    {
        logger.mode = mode;
    }

    if let Some(raw) = manager.get(LOG_LEVEL_KEY)
        && let Some(level) = LogLevel::parse(raw)
    {
        logger.min_level = level;
    }

    if let Some(raw) = manager.get(LOG_TIMEZONE_KEY)
        && let Some(tz) = TimeZone::parse(raw)
    {
        logger.timezone = tz;
    }

    if let Some(raw) = manager.get(LOG_TIMESTAMP_FORMAT_KEY) {
        if raw.trim().eq_ignore_ascii_case("custom") {
            let pattern = manager
                .get(LOG_TIMESTAMP_PATTERN_KEY)
                .unwrap_or("%Y-%m-%d %H:%M:%S")
                .to_string();
            logger.timestamp_format = TimestampFormat::Custom(pattern);
        } else {
            logger.timestamp_format = TimestampFormat::parse(raw);
        }
    } else if let Some(pattern) = manager.get(LOG_TIMESTAMP_PATTERN_KEY) {
        logger.timestamp_format = TimestampFormat::Custom(pattern.to_string());
    }

    logger
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawConfig {
    #[serde(default)]
    rust: Option<RawRustSection>,
    #[serde(default)]
    runtime: Option<RawRuntimeSection>,
    #[serde(default)]
    log: Option<RawLogSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawRustSection {
    #[serde(default)]
    runtime: Option<RawRuntimeSection>,
    #[serde(default)]
    log: Option<RawLogSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawRuntimeSection {
    #[serde(default)]
    worker_count: Option<usize>,
    #[serde(default)]
    ingress_queue: Option<usize>,
    #[serde(default)]
    worker_queue: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawLogSection {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    timestamp_format: Option<String>,
    #[serde(default)]
    timestamp_pattern: Option<String>,
}

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;
