use std::collections::VecDeque;
use std::env;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::logging_format::{format_level_tag, format_log_line, format_timestamp};

static CONSOLE_LOG_OUTPUT_ENABLED: AtomicBool = AtomicBool::new(true);
const LOG_BUFFER_CAPACITY: usize = 400;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BufferedLogEntry {
    pub timestamp: String,
    pub level: String,
    pub module: String,
    pub message: String,
    pub line: String,
}

#[derive(Debug)]
struct LogBuffer {
    entries: VecDeque<BufferedLogEntry>,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(LOG_BUFFER_CAPACITY),
        }
    }
}

impl LogBuffer {
    fn push(&mut self, entry: BufferedLogEntry) {
        if self.entries.len() >= LOG_BUFFER_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    fn recent(&self, limit: usize) -> Vec<BufferedLogEntry> {
        let take = limit.max(1);
        let skip = self.entries.len().saturating_sub(take);
        self.entries.iter().skip(skip).cloned().collect()
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.entries.clear();
    }
}

fn global_log_buffer() -> &'static Mutex<LogBuffer> {
    static LOG_BUFFER: OnceLock<Mutex<LogBuffer>> = OnceLock::new();
    LOG_BUFFER.get_or_init(|| Mutex::new(LogBuffer::default()))
}

fn push_buffered_log_entry(entry: BufferedLogEntry) {
    if let Ok(mut buffer) = global_log_buffer().lock() {
        buffer.push(entry);
    }
}

pub fn recent_buffered_logs(limit: usize) -> Vec<BufferedLogEntry> {
    global_log_buffer()
        .lock()
        .map(|buffer| buffer.recent(limit))
        .unwrap_or_default()
}

#[cfg(test)]
fn clear_buffered_logs() {
    if let Ok(mut buffer) = global_log_buffer().lock() {
        buffer.clear();
    }
}

pub fn set_console_log_output_enabled(enabled: bool) -> bool {
    CONSOLE_LOG_OUTPUT_ENABLED.swap(enabled, Ordering::SeqCst)
}

pub fn is_console_log_output_enabled() -> bool {
    CONSOLE_LOG_OUTPUT_ENABLED.load(Ordering::SeqCst)
}

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

pub fn emit_console_log(level: LogLevel, module: &str, message: impl AsRef<str>) {
    Logger::with_config(LoggerConfig::from_env()).log_with_module(level, module, message);
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

        let message = message.as_ref();
        let timestamp = self.format_system_time(SystemTime::now());
        let plain_level = level.as_str().to_string();
        let plain_line = format_log_line(&timestamp, plain_level.as_str(), module, message);
        push_buffered_log_entry(BufferedLogEntry {
            timestamp: timestamp.clone(),
            level: plain_level,
            module: module.to_string(),
            message: message.to_string(),
            line: plain_line,
        });

        if is_console_log_output_enabled() {
            let level_tag = format_level_tag(self.config.mode, level);
            let line = format_log_line(&timestamp, &level_tag, module, message);
            println!("{line}");
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn log_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn buffered_logs_capture_plain_text_even_when_console_output_is_disabled() {
        let _lock = log_test_lock()
            .lock()
            .expect("log test lock should not be poisoned");
        clear_buffered_logs();
        let previous = set_console_log_output_enabled(false);

        let logger = Logger::new(LogMode::Color, LogLevel::Debug);
        logger.info_in("web.host", "ready");

        let entries = recent_buffered_logs(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].level, "INFO");
        assert_eq!(entries[0].module, "web.host");
        assert_eq!(entries[0].message, "ready");
        assert!(entries[0].line.contains("web.host ready"));
        assert!(!entries[0].line.contains('\u{1b}'));

        set_console_log_output_enabled(previous);
        clear_buffered_logs();
    }

    #[test]
    fn buffered_logs_keep_the_latest_entries_within_capacity() {
        let _lock = log_test_lock()
            .lock()
            .expect("log test lock should not be poisoned");
        clear_buffered_logs();
        let previous = set_console_log_output_enabled(false);
        let logger = Logger::new(LogMode::Mono, LogLevel::Debug);

        for index in 0..(LOG_BUFFER_CAPACITY + 25) {
            logger.info_in("buffer.test", format!("entry {index}"));
        }

        let entries = recent_buffered_logs(5);
        assert_eq!(entries.len(), 5);
        assert_eq!(
            entries[0].message,
            format!("entry {}", LOG_BUFFER_CAPACITY + 20)
        );
        assert_eq!(
            entries[4].message,
            format!("entry {}", LOG_BUFFER_CAPACITY + 24)
        );

        set_console_log_output_enabled(previous);
        clear_buffered_logs();
    }
}
