use super::*;
use std::sync::{Mutex, OnceLock};

fn log_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
// 必要测试
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
// 必要测试
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
