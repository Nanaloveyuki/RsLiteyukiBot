use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use liteyukibot_core::{ManagedProcessSpec, ProcessManager, RestartPolicy};
use tokio::time::{sleep, timeout};

fn looping_runner(
    starts: Arc<AtomicUsize>,
) -> impl Fn(
    tokio::sync::watch::Receiver<bool>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
+ Clone
+ Send
+ Sync
+ 'static {
    move |mut shutdown_rx| {
        let starts = Arc::clone(&starts);
        Box::pin(async move {
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
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_manager_start_and_running_state() {
    let manager = ProcessManager::new();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            "worker-a",
            ManagedProcessSpec::new("worker-a"),
            looping_runner(starts),
        )
        .expect("register should succeed");

    manager.start_all().expect("start_all should succeed");
    sleep(Duration::from_millis(80)).await;
    assert!(manager.is_running("worker-a"));

    manager
        .terminate_all()
        .await
        .expect("terminate_all should succeed");
    assert!(!manager.is_running("worker-a"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_manager_restart_command_starts_new_generation() {
    let manager = ProcessManager::new();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            "worker-r",
            ManagedProcessSpec::new("worker-r"),
            looping_runner(Arc::clone(&starts)),
        )
        .expect("register should succeed");
    manager.start("worker-r").expect("start should succeed");

    timeout(Duration::from_secs(1), async {
        while starts.load(Ordering::SeqCst) < 1 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("initial start should happen");

    manager
        .restart("worker-r")
        .await
        .expect("restart should succeed");

    timeout(Duration::from_secs(1), async {
        while starts.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("restart should trigger new generation");

    manager
        .terminate_all()
        .await
        .expect("terminate_all should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_failure_policy_restarts_automatically() {
    let manager = ProcessManager::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_runner = Arc::clone(&attempts);

    let mut spec = ManagedProcessSpec::new("worker-fail");
    spec.restart_policy = RestartPolicy::OnFailure;
    spec.shutdown_timeout = Duration::from_secs(1);

    manager
        .register("worker-fail", spec, move |_shutdown_rx| {
            let attempts = Arc::clone(&attempts_for_runner);
            Box::pin(async move {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    return Err("fail once".to_string());
                }
                sleep(Duration::from_secs(5)).await;
                Ok(())
            })
        })
        .expect("register should succeed");

    manager.start("worker-fail").expect("start should succeed");

    timeout(Duration::from_secs(1), async {
        while attempts.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("OnFailure should restart after first failure");

    manager
        .terminate_all()
        .await
        .expect("terminate_all should succeed");
}
