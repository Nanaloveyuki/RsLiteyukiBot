use serde_json::json;
use tokio::sync::mpsc;

use super::*;
use crate::observability::{LogLevel, LogMode};

fn test_logger() -> Logger {
    Logger::with_config(LoggerConfig {
        mode: LogMode::Mono,
        min_level: LogLevel::Error,
        timezone: crate::observability::TimeZone::Utc,
        timestamp_format: crate::observability::TimestampFormat::EpochMillis,
    })
}

fn test_event(id: u64) -> BotEvent {
    BotEvent {
        id,
        topic: format!("topic-{id}"),
        payload: json!({ "id": id }),
        timestamp_ms: id as u128,
    }
}

#[tokio::test]
// 必要测试
async fn dispatch_round_robin_fast_path_rotates_cursor() {
    let logger = test_logger();
    let (tx0, mut rx0) = mpsc::channel::<BotEvent>(1);
    let (tx1, _rx1) = mpsc::channel::<BotEvent>(1);
    let senders = vec![tx0, tx1];
    let mut next_worker = 0usize;

    let result = dispatch_event_round_robin(test_event(1), &senders, &mut next_worker, &logger);
    assert!(result.is_ok());
    assert_eq!(next_worker, 1);

    let received = rx0.try_recv().expect("worker-0 should receive event");
    assert_eq!(received.id, 1);
}

#[tokio::test]
// 必要测试
async fn dispatch_falls_back_when_start_worker_full() {
    let logger = test_logger();
    let (tx0, _rx0) = mpsc::channel::<BotEvent>(1);
    let (tx1, mut rx1) = mpsc::channel::<BotEvent>(1);
    let senders = vec![tx0, tx1];
    let mut next_worker = 0usize;

    senders[0]
        .try_send(test_event(100))
        .expect("setup should fill worker-0 queue");

    let result = dispatch_event_round_robin(test_event(2), &senders, &mut next_worker, &logger);
    assert!(result.is_ok());
    assert_eq!(next_worker, 0);

    let received = rx1.try_recv().expect("worker-1 should receive fallback");
    assert_eq!(received.id, 2);
}

#[tokio::test]
// 必要测试
async fn dispatch_returns_event_when_all_workers_full() {
    let logger = test_logger();
    let (tx0, _rx0) = mpsc::channel::<BotEvent>(1);
    let (tx1, _rx1) = mpsc::channel::<BotEvent>(1);
    let senders = vec![tx0, tx1];
    let mut next_worker = 0usize;

    senders[0]
        .try_send(test_event(100))
        .expect("setup should fill worker-0 queue");
    senders[1]
        .try_send(test_event(101))
        .expect("setup should fill worker-1 queue");

    let result = dispatch_event_round_robin(test_event(3), &senders, &mut next_worker, &logger);
    let dropped = result.expect_err("event should be returned when all workers are full");
    assert_eq!(dropped.id, 3);
    assert_eq!(next_worker, 0);
}
