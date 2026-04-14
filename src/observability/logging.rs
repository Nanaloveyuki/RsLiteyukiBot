use std::env;
use std::fmt;
use std::time::SystemTime;

use chrono::{DateTime, Utc};

use super::logging_format::{format_level_tag, format_log_line, format_timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "debug" | "dbg" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" | "err" => Some(Self::Error),
            _ => None,
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str().trim())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    Mono,
    Color,
}

impl LogMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mono" | "bw" | "blackwhite" => Some(Self::Mono),
            "color" | "colour" | "ansi" => Some(Self::Color),
            _ => None,
        }
    }
}

impl fmt::Display for LogMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mono => f.write_str("mono"),
            Self::Color => f.write_str("color"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeZone {
    Local,
    Utc,
}

impl TimeZone {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Self::Local),
            "utc" | "z" => Some(Self::Utc),
            _ => None,
        }
    }
}

impl fmt::Display for TimeZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => f.write_str("local"),
            Self::Utc => f.write_str("utc"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimestampFormat {
    EpochSeconds,
    EpochMillis,
    Rfc3339,
    Custom(String),
}

impl TimestampFormat {
    pub fn parse(value: &str) -> Self {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "epoch_s" | "epoch_sec" | "epoch_seconds" => Self::EpochSeconds,
            "epoch_ms" | "epoch_millis" | "epoch_milliseconds" => Self::EpochMillis,
            "rfc3339" => Self::Rfc3339,
            _ => Self::Custom(value.trim().to_string()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::EpochSeconds => "epoch_s",
            Self::EpochMillis => "epoch_ms",
            Self::Rfc3339 => "rfc3339",
            Self::Custom(_) => "custom",
        }
    }
}

impl fmt::Display for TimestampFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Custom(pattern) => write!(f, "{}({})", self.name(), pattern),
            _ => f.write_str(self.name()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoggerConfig {
    pub mode: LogMode,
    pub min_level: LogLevel,
    pub timezone: TimeZone,
    pub timestamp_format: TimestampFormat,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            mode: LogMode::Color,
            min_level: LogLevel::Info,
            timezone: TimeZone::Local,
            timestamp_format: TimestampFormat::Custom("%Y-%m-%d %H:%M:%S".to_string()),
        }
    }
}

impl LoggerConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(value) = env::var("LY_LOG_MODE")
            && let Some(mode) = LogMode::parse(&value)
        {
            config.mode = mode;
        }

        if let Ok(value) = env::var("LY_LOG_LEVEL")
            && let Some(level) = LogLevel::parse(&value)
        {
            config.min_level = level;
        }

        if let Ok(value) = env::var("LY_LOG_TZ")
            && let Some(tz) = TimeZone::parse(&value)
        {
            config.timezone = tz;
        }

        if let Ok(value) = env::var("LY_LOG_TS_FORMAT") {
            if value.trim().eq_ignore_ascii_case("custom") {
                let pattern = env::var("LY_LOG_TS_PATTERN")
                    .unwrap_or_else(|_| "%Y-%m-%d %H:%M:%S".to_string());
                config.timestamp_format = TimestampFormat::Custom(pattern);
            } else {
                config.timestamp_format = TimestampFormat::parse(&value);
            }
        }

        config
    }
}

#[derive(Debug, Clone)]
pub struct Logger {
    config: LoggerConfig,
}

impl Logger {
    pub fn new(mode: LogMode, min_level: LogLevel) -> Self {
        Self::with_config(LoggerConfig {
            mode,
            min_level,
            ..LoggerConfig::default()
        })
    }

    pub fn with_config(config: LoggerConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &LoggerConfig {
        &self.config
    }

    pub fn debug(&self, message: impl AsRef<str>) {
        self.log(LogLevel::Debug, message);
    }

    pub fn debug_in(&self, module: &str, message: impl AsRef<str>) {
        self.log_with_module(LogLevel::Debug, module, message);
    }

    pub fn info(&self, message: impl AsRef<str>) {
        self.log(LogLevel::Info, message);
    }

    pub fn info_in(&self, module: &str, message: impl AsRef<str>) {
        self.log_with_module(LogLevel::Info, module, message);
    }

    pub fn warn(&self, message: impl AsRef<str>) {
        self.log(LogLevel::Warn, message);
    }

    pub fn warn_in(&self, module: &str, message: impl AsRef<str>) {
        self.log_with_module(LogLevel::Warn, module, message);
    }

    pub fn error(&self, message: impl AsRef<str>) {
        self.log(LogLevel::Error, message);
    }

    pub fn error_in(&self, module: &str, message: impl AsRef<str>) {
        self.log_with_module(LogLevel::Error, module, message);
    }

    pub fn log(&self, level: LogLevel, message: impl AsRef<str>) {
        self.log_with_module(level, "global", message);
    }

    pub fn log_with_module(&self, level: LogLevel, module: &str, message: impl AsRef<str>) {
        if level < self.config.min_level {
            return;
        }

        let timestamp = self.format_system_time(SystemTime::now());
        let level_tag = format_level_tag(self.config.mode, level);
        let line = format_log_line(&timestamp, &level_tag, module, message.as_ref());
        println!("{line}");
    }

    pub fn format_unix_seconds(&self, seconds: i64) -> String {
        match DateTime::<Utc>::from_timestamp(seconds, 0) {
            Some(dt_utc) => self.format_datetime(dt_utc),
            None => format!("invalid_unix_seconds({seconds})"),
        }
    }

    pub fn format_unix_millis(&self, millis: i64) -> String {
        let seconds = millis.div_euclid(1000);
        let nanos = (millis.rem_euclid(1000) as u32) * 1_000_000;
        match DateTime::<Utc>::from_timestamp(seconds, nanos) {
            Some(dt_utc) => self.format_datetime(dt_utc),
            None => format!("invalid_unix_millis({millis})"),
        }
    }

    pub fn format_system_time(&self, timestamp: SystemTime) -> String {
        let dt_utc: DateTime<Utc> = timestamp.into();
        self.format_datetime(dt_utc)
    }

    fn format_datetime(&self, dt_utc: DateTime<Utc>) -> String {
        format_timestamp(dt_utc, self.config.timezone, &self.config.timestamp_format)
    }
}
