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
fn epoch_seconds_format_matches_unix_timestamp() {
    let logger = make_logger(TimestampFormat::EpochSeconds);
    assert_eq!(logger.format_unix_seconds(1_700_000_000), "1700000000");
}

#[test]
fn epoch_millis_format_rounds_to_millis() {
    let logger = make_logger(TimestampFormat::EpochMillis);
    assert_eq!(logger.format_unix_millis(1_700_000_123), "1700000123");
}

#[test]
fn rfc3339_format_uses_utc_millis() {
    let logger = make_logger(TimestampFormat::Rfc3339);
    assert_eq!(logger.format_unix_seconds(1), "1970-01-01T00:00:01.000Z");
}

#[test]
fn custom_format_respects_pattern_for_millis() {
    let logger = make_logger(TimestampFormat::Custom("%Y/%m/%d %H:%M:%S".into()));
    assert_eq!(logger.format_unix_millis(12_345), "1970/01/01 00:00:12");
}

#[test]
fn format_unix_seconds_reports_invalid_range() {
    let logger = make_logger(TimestampFormat::EpochSeconds);
    let invalid = i64::MAX;
    assert_eq!(
        logger.format_unix_seconds(invalid),
        format!("invalid_unix_seconds({invalid})")
    );
}

#[test]
fn format_unix_millis_reports_invalid_range() {
    let logger = make_logger(TimestampFormat::EpochMillis);
    let invalid = i64::MAX;
    assert_eq!(
        logger.format_unix_millis(invalid),
        format!("invalid_unix_millis({invalid})")
    );
}
