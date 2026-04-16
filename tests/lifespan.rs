use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use liteyukibot_core::{
    HookFilter, LifecycleContext, LifecycleFailurePolicy, LifecyclePhase, Lifespan, RuntimeFlavor,
};
use tokio::sync::Barrier;
use tokio::time::{Duration, sleep, timeout};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_hooks_share_context_state() {
    let mut lifespan = Lifespan::new();
    let counter = Arc::new(AtomicU32::new(0));

    let before_counter = Arc::clone(&counter);
    lifespan.on_before_start_sync("before", HookFilter::default(), move |context| {
        context.set_meta("phase", "before_start");
        before_counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });

    let after_counter = Arc::clone(&counter);
    lifespan.on_after_start("after", HookFilter::default(), move |context| {
        let after_counter = Arc::clone(&after_counter);
        async move {
            let phase = context.get_meta("phase").unwrap_or_default();
            if phase != "before_start" {
                return Err("shared context metadata missing".to_string());
            }
            context.set_meta("phase", "after_start");
            after_counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });

    let context = Arc::new(LifecycleContext::new(
        "liteyuki",
        "0.1.0",
        RuntimeFlavor::Cli,
    ));
    lifespan
        .before_start(context.clone())
        .await
        .expect("before_start should succeed");
    lifespan
        .after_start(context.clone())
        .await
        .expect("after_start should succeed");

    assert_eq!(counter.load(Ordering::SeqCst), 2);
    assert_eq!(context.get_meta("phase"), Some("after_start".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_filter_respects_runtime_capabilities() {
    let mut lifespan = Lifespan::new();
    let hit_count = Arc::new(AtomicU32::new(0));

    let llm_hits = Arc::clone(&hit_count);
    lifespan.on_before_start_sync(
        "llm-only",
        HookFilter {
            require_llm: true,
            ..HookFilter::default()
        },
        move |_context| {
            llm_hits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    );

    let context_cli = Arc::new(LifecycleContext::new(
        "liteyuki",
        "0.1.0",
        RuntimeFlavor::Cli,
    ));
    lifespan
        .before_start(context_cli)
        .await
        .expect("cli context should still succeed");
    assert_eq!(hit_count.load(Ordering::SeqCst), 0);

    let context_llm = Arc::new(LifecycleContext::new(
        "liteyuki",
        "0.1.0",
        RuntimeFlavor::Llm,
    ));
    lifespan
        .before_start(context_llm)
        .await
        .expect("llm context should succeed");
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hooks_in_same_phase_run_concurrently() {
    let mut lifespan = Lifespan::new();
    lifespan.set_failure_policy(LifecycleFailurePolicy::Continue);
    let barrier = Arc::new(Barrier::new(2));

    let barrier_a = Arc::clone(&barrier);
    lifespan.on_before_start("hook-a", HookFilter::default(), move |_context| {
        let barrier = Arc::clone(&barrier_a);
        async move {
            barrier.wait().await;
            Ok(())
        }
    });

    let barrier_b = Arc::clone(&barrier);
    lifespan.on_before_start("hook-b", HookFilter::default(), move |_context| {
        let barrier = Arc::clone(&barrier_b);
        async move {
            barrier.wait().await;
            Ok(())
        }
    });

    let context = Arc::new(LifecycleContext::new(
        "liteyuki",
        "0.1.0",
        RuntimeFlavor::Cli,
    ));
    let result = timeout(Duration::from_millis(300), lifespan.before_start(context)).await;
    assert!(
        result.is_ok(),
        "before_start should finish when hooks are scheduled concurrently"
    );
    assert!(
        result.expect("timeout should not fire").is_ok(),
        "hooks should complete without failures"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fail_fast_stops_at_first_error() {
    let mut lifespan = Lifespan::new();
    lifespan.set_failure_policy(LifecycleFailurePolicy::FailFast);

    let second_hook_hits = Arc::new(AtomicU32::new(0));

    lifespan.on_before_start("first-fail", HookFilter::default(), |_context| async move {
        sleep(Duration::from_millis(100)).await;
        Err("first failed".to_string())
    });

    let second_hook_hits_clone = Arc::clone(&second_hook_hits);
    lifespan.on_before_start_sync("second", HookFilter::default(), move |_context| {
        second_hook_hits_clone.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });

    let context = Arc::new(LifecycleContext::new(
        "liteyuki",
        "0.1.0",
        RuntimeFlavor::Cli,
    ));
    let err = lifespan
        .before_start(context)
        .await
        .expect_err("fail_fast should return error");

    assert_eq!(err.phase, LifecyclePhase::BeforeStart);
    assert_eq!(err.failures.len(), 1);
    assert_eq!(err.failures[0].hook_name, "first-fail");
    assert_eq!(second_hook_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_timeout_is_reported_as_failure() {
    let mut lifespan = Lifespan::new();
    lifespan.set_failure_policy(LifecycleFailurePolicy::FailFast);
    lifespan.set_hook_timeout(Some(Duration::from_millis(50)));

    lifespan.on_before_start("slow-hook", HookFilter::default(), |_context| async move {
        sleep(Duration::from_millis(200)).await;
        Ok(())
    });

    let context = Arc::new(LifecycleContext::new(
        "liteyuki",
        "0.1.0",
        RuntimeFlavor::Cli,
    ));
    let err = lifespan
        .before_start(context)
        .await
        .expect_err("timeout should be reported as lifecycle failure");

    assert_eq!(err.phase, LifecyclePhase::BeforeStart);
    assert_eq!(err.failures.len(), 1);
    assert_eq!(err.failures[0].hook_name, "slow-hook");
    assert!(
        err.failures[0].reason.contains("timed out"),
        "timeout reason should mention timeout"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn continue_policy_collects_all_errors() {
    let mut lifespan = Lifespan::new();
    lifespan.set_failure_policy(LifecycleFailurePolicy::Continue);

    lifespan.on_before_process_shutdown_sync(
        "fail-1",
        HookFilter::default(),
        |_context, _process_name| Err("first failure".to_string()),
    );
    lifespan.on_before_process_shutdown_sync(
        "fail-2",
        HookFilter::default(),
        |_context, _process_name| Err("second failure".to_string()),
    );

    let context = Arc::new(LifecycleContext::new(
        "liteyuki",
        "0.1.0",
        RuntimeFlavor::Docker,
    ));
    let err = lifespan
        .before_process_shutdown(context, Arc::<str>::from("worker-1"))
        .await
        .expect_err("continue policy should aggregate failures");

    assert_eq!(err.phase, LifecyclePhase::BeforeProcessShutdown);
    assert_eq!(err.failures.len(), 2);
}
