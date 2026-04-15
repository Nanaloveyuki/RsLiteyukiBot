use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use liteyukibot_core::{
    AdapterConfig, AdapterEndpoint, AdapterRoute, AdapterTransport, BotEvent, BotRuntimeConfig,
    HookFilter, LiteyukiBot, LiteyukiBotError, ManagedProcessSpec, RuntimeTarget,
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

#[test]
fn runtime_target_maps_capabilities_and_tunes_config() {
    let target = RuntimeTarget::DockerWeb;
    let capabilities = target.capabilities();
    assert!(capabilities.web);
    assert!(capabilities.docker);
    assert!(!capabilities.cli);

    let tuned = target.tune_runtime_config(BotRuntimeConfig::default());
    assert!(tuned.worker_count >= 2);
    assert!(tuned.ingress_queue >= 2048);
    assert!(tuned.worker_queue >= 512);

    let cli_web = RuntimeTarget::CliWeb.capabilities();
    assert!(cli_web.cli);
    assert!(cli_web.web);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn liteyuki_bot_orchestrates_runtime_and_managed_processes() {
    let (processed_tx, mut processed_rx) = mpsc::unbounded_channel::<u64>();
    let starts = Arc::new(AtomicUsize::new(0));
    let starts_for_runner = Arc::clone(&starts);

    let mut bot = LiteyukiBot::builder("rs-liteyuki", "0.2.0")
        .with_target(RuntimeTarget::CliWeb)
        .with_event_handler(move |event, _logger| {
            let processed_tx = processed_tx.clone();
            async move {
                let _ = processed_tx.send(event.id);
            }
        })
        .build();

    bot.on_before_start_sync("meta", HookFilter::default(), |context| {
        context.set_meta("boot.mode", "integration");
        Ok(())
    });

    let process_name = "heartbeat";
    let process_spec = ManagedProcessSpec::new(process_name);
    bot.register_process(process_name, process_spec, move |mut shutdown_rx| {
        let starts = Arc::clone(&starts_for_runner);
        async move {
            starts.fetch_add(1, Ordering::SeqCst);
            loop {
                if *shutdown_rx.borrow() {
                    break;
                }
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
            Ok(())
        }
    })
    .expect("register process should succeed");

    bot.start().await.expect("bot should start");
    assert_eq!(
        bot.lifecycle_context().get_meta("boot.mode"),
        Some("integration".to_string())
    );
    assert_eq!(bot.target(), RuntimeTarget::CliWeb);

    bot.send(BotEvent::new(7, "integration.event", json!({ "ok": true })))
        .await
        .expect("send should succeed");

    let processed = timeout(Duration::from_secs(1), processed_rx.recv())
        .await
        .expect("event handling should not timeout")
        .expect("processed channel should have value");
    assert_eq!(processed, 7);

    timeout(Duration::from_secs(1), async {
        while starts.load(Ordering::SeqCst) < 1 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("managed process should start");

    bot.restart_process(process_name)
        .await
        .expect("process restart should succeed");

    timeout(Duration::from_secs(1), async {
        while starts.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("managed process should restart");

    bot.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bot_start_adapter_failure_rolls_back_runtime_and_processes() {
    let process_name = "rollback-heartbeat";
    let mut ws_invalid = AdapterConfig::default();
    ws_invalid.id = "ws-invalid".to_string();
    ws_invalid.transport = AdapterTransport::WebSocketForward;
    ws_invalid.endpoint = AdapterEndpoint {
        url: "not-a-valid-ws-url".to_string(),
        headers: Default::default(),
        token: None,
        timeout_ms: 500,
    };
    ws_invalid.route = AdapterRoute::default();
    ws_invalid.queue_capacity = 4;
    ws_invalid.max_payload_size = None;
    ws_invalid.max_connections = None;

    let mut bot = LiteyukiBot::builder("rs-liteyuki", "0.2.0")
        .with_target(RuntimeTarget::CliWeb)
        .with_adapter_configs(vec![ws_invalid])
        .with_adapter_autostart(true)
        .build();

    bot.register_process(
        process_name,
        ManagedProcessSpec::new(process_name),
        move |mut shutdown_rx| async move {
            loop {
                if *shutdown_rx.borrow() {
                    break;
                }
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
            Ok(())
        },
    )
    .expect("register process should succeed");

    let start_err = bot
        .start()
        .await
        .expect_err("invalid adapter should fail startup");
    assert!(
        matches!(start_err, LiteyukiBotError::Adapter(_)),
        "expected adapter error, got {start_err}"
    );

    let send_err = bot
        .send(BotEvent::new(11, "integration.rollback", json!({ "ok": true })))
        .await
        .expect_err("runtime should be rolled back");
    assert!(matches!(send_err, LiteyukiBotError::NotStarted));
    assert!(
        !bot.process_manager().is_running(process_name),
        "process should be terminated by rollback"
    );

    let shutdown_err = bot
        .shutdown()
        .await
        .expect_err("shutdown after rollback should report not started");
    assert!(matches!(shutdown_err, LiteyukiBotError::NotStarted));
}
