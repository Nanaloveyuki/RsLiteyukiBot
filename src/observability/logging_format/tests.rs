use chrono::DateTime;

use super::*;

#[test]
// 必要测试
fn mono_level_tag_uses_plain_text() {
    assert_eq!(format_level_tag(LogMode::Mono, LogLevel::Warn), "WARN");
}

#[test]
// 必要测试
fn color_level_tag_wraps_with_ansi_sequence() {
    assert_eq!(
        format_level_tag(LogMode::Color, LogLevel::Error),
        "\x1b[31mERROR\x1b[0m"
    );
}

#[test]
// 必要测试
fn rfc3339_timestamp_uses_utc_suffix() {
    let dt = DateTime::<Utc>::from_timestamp(1, 0).expect("valid unix seconds");
    assert_eq!(
        format_timestamp(dt, TimeZone::Utc, &TimestampFormat::Rfc3339),
        "1970-01-01T00:00:01.000Z"
    );
}

#[test]
// 必要测试
fn log_line_uses_default_template() {
    assert_eq!(
        format_log_line("2026-04-13 20:00:00", "INFO", "core.runtime", "booted"),
        "2026-04-13 20:00:00 | INFO core.runtime booted"
    );
}
