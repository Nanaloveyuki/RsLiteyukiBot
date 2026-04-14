use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use liteyukibot_core::{BotEvent, Rule, SessionEvent, SessionRouter};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_router_respects_priority_and_block() {
    let router = SessionRouter::new();
    let hit = Arc::new(AtomicUsize::new(0));

    let high = Arc::clone(&hit);
    router.on_keywords("high", vec!["hello"], 10, true, move |_event| {
        let high = Arc::clone(&high);
        async move {
            high.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });

    let low = Arc::clone(&hit);
    router.on_keywords("low", vec!["hello"], 1, false, move |_event| {
        let low = Arc::clone(&low);
        async move {
            low.fetch_add(100, Ordering::SeqCst);
            Ok(())
        }
    });

    let event = SessionEvent::from_bot_event(&BotEvent::new(
        1,
        "chat.private",
        json!({ "text": "hello world", "user_id": "u1" }),
    ));
    let report = router.dispatch(event).await;
    assert_eq!(report.matched, 1);
    assert_eq!(report.handled, 1);
    assert!(report.blocked);
    assert_eq!(hit.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_router_supports_rule_composition() {
    let router = SessionRouter::new();
    let hits = Arc::new(AtomicUsize::new(0));

    let rule = Rule::keywords(vec!["ping"]).and(Rule::new("topic", |event| async move {
        event.topic.as_ref() == "chat.group"
    }));

    let hits_for_handler = Arc::clone(&hits);
    router.on_message("composed", rule, 5, false, move |_event| {
        let hits_for_handler = Arc::clone(&hits_for_handler);
        async move {
            hits_for_handler.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });

    let event_fail = SessionEvent::from_bot_event(&BotEvent::new(
        1,
        "chat.private",
        json!({ "text": "ping", "user_id": "u1" }),
    ));
    router.dispatch(event_fail).await;
    assert_eq!(hits.load(Ordering::SeqCst), 0);

    let event_ok = SessionEvent::from_bot_event(&BotEvent::new(
        2,
        "chat.group",
        json!({ "text": "ping from group", "user_id": "u2" }),
    ));
    let report = router.dispatch(event_ok).await;
    assert_eq!(report.matched, 1);
    assert_eq!(report.handled, 1);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}
