use chrono::{DateTime, Local, SecondsFormat, Utc};

use super::logging::{LogLevel, LogMode, TimeZone, TimestampFormat};

pub(crate) fn format_level_tag(mode: LogMode, level: LogLevel) -> String {
    match mode {
        LogMode::Mono => level.as_str().to_string(),
        LogMode::Color => {
            let color = match level {
                LogLevel::Debug => 36, // cyan
                LogLevel::Info => 32,  // green
                LogLevel::Warn => 33,  // yellow
                LogLevel::Error => 31, // red
            };
            format!("\x1b[{}m{}\x1b[0m", color, level.as_str())
        }
    }
}

pub(crate) fn format_log_line(
    timestamp: &str,
    level_tag: &str,
    module_name: &str,
    message: &str,
) -> String {
    format!("{timestamp} | {level_tag} {module_name} {message}")
}

pub(crate) fn format_timestamp(
    dt_utc: DateTime<Utc>,
    timezone: TimeZone,
    timestamp_format: &TimestampFormat,
) -> String {
    match timestamp_format {
        TimestampFormat::EpochSeconds => dt_utc.timestamp().to_string(),
        TimestampFormat::EpochMillis => dt_utc.timestamp_millis().to_string(),
        TimestampFormat::Rfc3339 => match timezone {
            TimeZone::Utc => dt_utc.to_rfc3339_opts(SecondsFormat::Millis, true),
            TimeZone::Local => dt_utc
                .with_timezone(&Local)
                .to_rfc3339_opts(SecondsFormat::Millis, false),
        },
        TimestampFormat::Custom(pattern) => match timezone {
            TimeZone::Utc => dt_utc.format(pattern).to_string(),
            TimeZone::Local => dt_utc.with_timezone(&Local).format(pattern).to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;

    #[test]
    fn mono_level_tag_uses_plain_text() {
        assert_eq!(format_level_tag(LogMode::Mono, LogLevel::Warn), "WARN");
    }

    #[test]
    fn color_level_tag_wraps_with_ansi_sequence() {
        assert_eq!(
            format_level_tag(LogMode::Color, LogLevel::Error),
            "\x1b[31mERROR\x1b[0m"
        );
    }

    #[test]
    fn rfc3339_timestamp_uses_utc_suffix() {
        let dt = DateTime::<Utc>::from_timestamp(1, 0).expect("valid unix seconds");
        assert_eq!(
            format_timestamp(dt, TimeZone::Utc, &TimestampFormat::Rfc3339),
            "1970-01-01T00:00:01.000Z"
        );
    }

    #[test]
    fn log_line_uses_default_template() {
        assert_eq!(
            format_log_line("2026-04-13 20:00:00", "INFO", "core.runtime", "booted"),
            "2026-04-13 20:00:00 | INFO core.runtime booted"
        );
    }
}
