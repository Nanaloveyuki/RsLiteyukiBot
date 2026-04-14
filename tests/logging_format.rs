use liteyukibot_core::{LogLevel, LogMode, Logger, LoggerConfig, TimeZone, TimestampFormat};

fn make_logger(timestamp_format: TimestampFormat) -> Logger {
    Logger::with_config(LoggerConfig {
        mode: LogMode::Mono,
        min_level: LogLevel::Debug,
        timezone: TimeZone::Utc,
        timestamp_format,
    })
}

#[test]
fn level_tags_use_plain_level_names() {
    assert_eq!(LogLevel::Info.as_str(), "INFO");
    assert_eq!(LogLevel::Warn.as_str(), "WARN");
}

#[test]
fn level_display_trims_tag_padding() {
    assert_eq!(LogLevel::Info.to_string(), "INFO");
    assert_eq!(LogLevel::Warn.to_string(), "WARN");
}

#[test]
fn utc_rfc3339_timestamp_uses_millis() {
    let logger = make_logger(TimestampFormat::Rfc3339);
    assert_eq!(logger.format_unix_seconds(1), "1970-01-01T00:00:01.000Z");
}
