use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use liteyukibot_core::{ManagedProcessSpec, ProcessManager, RestartPolicy};
use tokio::task::JoinSet;
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
    assert!(
        manager
            .running_process_names()
            .iter()
            .any(|name| name.as_ref() == "worker-a"),
        "running process list should include worker-a"
    );

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
async fn process_manager_restart_storm_does_not_block_callers() {
    let manager = ProcessManager::new();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            "worker-storm",
            ManagedProcessSpec::new("worker-storm"),
            looping_runner(Arc::clone(&starts)),
        )
        .expect("register should succeed");
    manager.start("worker-storm").expect("start should succeed");

    timeout(Duration::from_secs(1), async {
        while starts.load(Ordering::SeqCst) < 1 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("initial start should happen");

    timeout(Duration::from_millis(250), async {
        let mut join_set = JoinSet::new();
        for _ in 0..1024 {
            let manager = manager.clone();
            join_set.spawn(async move { manager.restart("worker-storm").await });
        }

        while let Some(result) = join_set.join_next().await {
            result
                .expect("restart task should complete")
                .expect("restart should not fail under storm");
        }
    })
    .await
    .expect("restart storm should not block callers");

    manager
        .terminate_all()
        .await
        .expect("terminate_all should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_manager_restart_waits_until_new_generation_starts() {
    let manager = ProcessManager::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let generations = Arc::new(AtomicUsize::new(0));
    let starts_for_runner = Arc::clone(&starts);
    let generations_for_runner = Arc::clone(&generations);

    manager
        .register(
            "worker-order",
            ManagedProcessSpec::new("worker-order"),
            move |mut shutdown_rx| {
                let starts = Arc::clone(&starts_for_runner);
                let generations = Arc::clone(&generations_for_runner);
                Box::pin(async move {
                    let generation = generations.fetch_add(1, Ordering::SeqCst);
                    if generation > 0 {
                        sleep(Duration::from_millis(200)).await;
                    }
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
            },
        )
        .expect("register should succeed");
    manager.start("worker-order").expect("start should succeed");

    timeout(Duration::from_secs(1), async {
        while starts.load(Ordering::SeqCst) < 1 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("initial generation should start");

    let begin = Instant::now();
    manager
        .restart("worker-order")
        .await
        .expect("restart should wait until the next generation is spawned");
    assert!(
        begin.elapsed() < Duration::from_millis(180),
        "restart should not block on the new generation's internal warmup delay"
    );
    timeout(Duration::from_secs(1), async {
        while starts.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("new generation should reach its running state shortly after restart returns");

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
