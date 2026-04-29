pub(crate) const RUNTIME_WORKER_COUNT_KEY: &str = "LY_WORKERS";
pub(crate) const RUNTIME_INGRESS_QUEUE_KEY: &str = "LY_INGRESS_QUEUE";
pub(crate) const RUNTIME_WORKER_QUEUE_KEY: &str = "LY_WORKER_QUEUE";
pub(crate) const LOG_MODE_KEY: &str = "LY_LOG_MODE";
pub(crate) const LOG_LEVEL_KEY: &str = "LY_LOG_LEVEL";
pub(crate) const LOG_TIMEZONE_KEY: &str = "LY_LOG_TZ";
pub(crate) const LOG_TIMESTAMP_FORMAT_KEY: &str = "LY_LOG_TS_FORMAT";
pub(crate) const LOG_TIMESTAMP_PATTERN_KEY: &str = "LY_LOG_TS_PATTERN";

pub(crate) fn runtime_setting_value_pairs(
    worker_count: Option<usize>,
    ingress_queue: Option<usize>,
    worker_queue: Option<usize>,
    mode: Option<&str>,
    level: Option<&str>,
    timezone: Option<&str>,
    timestamp_format: Option<&str>,
    timestamp_pattern: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut values = Vec::new();

    if let Some(worker_count) = worker_count {
        values.push((RUNTIME_WORKER_COUNT_KEY, worker_count.to_string()));
    }
    if let Some(ingress_queue) = ingress_queue {
        values.push((RUNTIME_INGRESS_QUEUE_KEY, ingress_queue.to_string()));
    }
    if let Some(worker_queue) = worker_queue {
        values.push((RUNTIME_WORKER_QUEUE_KEY, worker_queue.to_string()));
    }
    if let Some(mode) = mode {
        values.push((LOG_MODE_KEY, mode.to_string()));
    }
    if let Some(level) = level {
        values.push((LOG_LEVEL_KEY, level.to_string()));
    }
    if let Some(timezone) = timezone {
        values.push((LOG_TIMEZONE_KEY, timezone.to_string()));
    }
    if let Some(timestamp_format) = timestamp_format {
        values.push((LOG_TIMESTAMP_FORMAT_KEY, timestamp_format.to_string()));
    }
    if let Some(timestamp_pattern) = timestamp_pattern {
        if timestamp_format.is_none() {
            values.push((LOG_TIMESTAMP_FORMAT_KEY, "custom".to_string()));
        }
        values.push((LOG_TIMESTAMP_PATTERN_KEY, timestamp_pattern.to_string()));
    }

    values
}
