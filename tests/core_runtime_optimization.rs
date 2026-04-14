use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use liteyukibot_core::core::formatting::{
    EventTextFormatter, format_event_text, format_event_with,
};
use liteyukibot_core::{
    BotEvent, BotRuntime, BotRuntimeConfig, LogLevel, LogMode, LoggerConfig, TimeZone,
    TimestampFormat,
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

fn test_logger_config() -> LoggerConfig {
    LoggerConfig {
        mode: LogMode::Mono,
        min_level: LogLevel::Error,
        timezone: TimeZone::Utc,
        timestamp_format: TimestampFormat::EpochMillis,
    }
}

fn test_event(id: u64) -> BotEvent {
    BotEvent {
        id,
        topic: format!("topic-{id}"),
        payload: json!({ "id": id }),
        timestamp_ms: id as u128,
    }
}

struct TopicOnlyFormatter;

impl EventTextFormatter for TopicOnlyFormatter {
    fn format_event(&self, event: &BotEvent) -> String {
        format!("topic={}", event.topic)
    }
}

#[test]
fn default_formatter_keeps_legacy_event_text_shape() {
    let event = BotEvent {
        id: 42,
        topic: "ping".to_string(),
        payload: json!({ "ok": true }),
        timestamp_ms: 1700000000000,
    };

    assert_eq!(
        format_event_text(&event),
        r#"event id=42 topic=ping payload={"ok":true}"#
    );
}

#[test]
fn formatter_trait_supports_custom_extensions() {
    let event = test_event(7);
    let formatted = format_event_with(&TopicOnlyFormatter, &event);
    assert_eq!(formatted, "topic=topic-7");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_processes_all_events_when_worker_queues_are_sufficient() {
    let (processed_tx, mut processed_rx) = mpsc::unbounded_channel::<u64>();
    let processed_total = Arc::new(AtomicUsize::new(0));
    let processed_total_for_handler = Arc::clone(&processed_total);

    let runtime = BotRuntime::with_handler(
        BotRuntimeConfig {
            worker_count: 2,
            ingress_queue: 64,
            worker_queue: 32,
            logger: test_logger_config(),
        },
        move |event, _logger| {
            let processed_tx = processed_tx.clone();
            let processed_total_for_handler = Arc::clone(&processed_total_for_handler);
            async move {
                processed_total_for_handler.fetch_add(1, Ordering::SeqCst);
                let _ = processed_tx.send(event.id);
            }
        },
    );

    let handle = runtime.start();
    let total = 20u64;
    for id in 0..total {
        handle
            .send(test_event(id))
            .await
            .expect("ingress send should succeed");
    }

    let mut received = Vec::with_capacity(total as usize);
    while received.len() < total as usize {
        let next = timeout(Duration::from_secs(2), processed_rx.recv())
            .await
            .expect("timed out waiting for processed event")
            .expect("processed channel should stay open while runtime is active");
        received.push(next);
    }
    received.sort_unstable();

    assert_eq!(received.len(), total as usize);
    assert_eq!(received, (0..total).collect::<Vec<_>>());
    assert_eq!(processed_total.load(Ordering::SeqCst), total as usize);

    handle.shutdown().await;
}
