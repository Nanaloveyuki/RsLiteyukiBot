use std::sync::Arc;
use std::time::Duration;

use liteyukibot_core::{
    BotEvent, BotRuntime, HookFilter, LifecycleContext, Lifespan, RuntimeSettings,
};
use serde_json::json;

const MODULE_MAIN: &str = "app.main";

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let settings = match RuntimeSettings::try_load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("failed to load runtime config from file/env, fallback to default: {err}");
            RuntimeSettings::default()
        }
    };
    let _ = settings.clone().install_global();
    let active_settings = RuntimeSettings::global_or_default();
    let runtime_config = RuntimeSettings::global_runtime_config().clone();

    let runtime = BotRuntime::with_handler(runtime_config, |event, logger| async move {
        logger.info_in(
            MODULE_MAIN,
            format!(
                "handled event_id={} topic={} payload={}",
                event.id, event.topic, event.payload
            ),
        );
        tokio::time::sleep(Duration::from_millis(80)).await;
    });

    let logger = runtime.logger();
    let mut lifespan = Lifespan::with_logger(logger.clone());
    lifespan.on_before_start_sync("init-metadata", HookFilter::default(), |context| {
        context.set_meta("boot.source", "rust-main");
        context.set_meta("runtime.flavor", format!("{:?}", context.runtime_flavor()));
        Ok(())
    });
    lifespan.on_after_start_sync("announce-capability", HookFilter::default(), |context| {
        context.set_meta("llm.enabled", context.capabilities().llm.to_string());
        Ok(())
    });
    lifespan.on_before_process_shutdown_sync(
        "mark-shutdown",
        HookFilter::default(),
        |context, process_name| {
            context.set_meta("shutdown.requested", "true");
            context.set_meta("shutdown.process", process_name.to_string());
            Ok(())
        },
    );
    let lifecycle_context = Arc::new(LifecycleContext::from_env(
        "liteyukibot-core",
        env!("CARGO_PKG_VERSION"),
    ));

    if let Err(err) = lifespan.before_start(lifecycle_context.clone()).await {
        logger.error_in(MODULE_MAIN, format!("lifespan before_start failed: {err}"));
    }
    logger.info_in(
        MODULE_MAIN,
        format!("runtime settings: {}", active_settings.describe()),
    );
    logger.info_in(
        MODULE_MAIN,
        format!(
            "timestamp conversion sample: 1700000000s => {}, 1700000000000ms => {}",
            logger.format_unix_seconds(1_700_000_000),
            logger.format_unix_millis(1_700_000_000_000)
        ),
    );

    let handle = runtime.start();
    if let Err(err) = lifespan.after_start(lifecycle_context.clone()).await {
        logger.error_in(MODULE_MAIN, format!("lifespan after_start failed: {err}"));
    }

    for id in 0..10_u64 {
        let event = BotEvent::new(
            id,
            "demo.message",
            json!({ "text": format!("hello-{}", id) }),
        );
        if let Err(err) = handle.send(event).await {
            logger.error_in(MODULE_MAIN, format!("send failed: {}", err));
            break;
        }
    }

    tokio::time::sleep(Duration::from_millis(900)).await;
    if let Err(err) = lifespan
        .before_process_shutdown(lifecycle_context.clone(), Arc::<str>::from("runtime"))
        .await
    {
        logger.error_in(
            MODULE_MAIN,
            format!("lifespan before_process_shutdown failed: {err}"),
        );
    }
    handle.shutdown().await;
    if let Err(err) = lifespan.after_shutdown(lifecycle_context).await {
        logger.error_in(
            MODULE_MAIN,
            format!("lifespan after_shutdown failed: {err}"),
        );
    }
}
