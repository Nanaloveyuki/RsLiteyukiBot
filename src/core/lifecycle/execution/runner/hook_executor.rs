use std::sync::Arc;

use tokio::time::timeout;

use super::*;

impl Lifespan {
    pub(super) async fn execute_hook(
        &self,
        phase: LifecyclePhase,
        name: String,
        handler: HookHandler,
        context: Arc<LifecycleContext>,
    ) -> Result<(), HookFailure> {
        Self::execute_hook_impl(
            phase,
            name,
            "start".to_string(),
            self.logger.clone(),
            self.hook_timeout,
            (handler)(context),
        )
        .await
    }

    pub(super) async fn execute_process_hook(
        &self,
        phase: LifecyclePhase,
        name: String,
        handler: ProcessHookHandler,
        context: Arc<LifecycleContext>,
        process_name: Arc<str>,
    ) -> Result<(), HookFailure> {
        Self::execute_hook_impl(
            phase,
            name,
            format!("process={} start", process_name),
            self.logger.clone(),
            self.hook_timeout,
            (handler)(context, process_name),
        )
        .await
    }

    pub(super) async fn execute_hook_impl(
        phase: LifecyclePhase,
        name: String,
        start_suffix: String,
        logger: Option<Logger>,
        hook_timeout: Option<Duration>,
        future: HookFuture,
    ) -> Result<(), HookFailure> {
        if let Some(logger) = &logger {
            logger.debug_in(
                MODULE_LIFECYCLE,
                format!("phase={phase:?} hook={name} {start_suffix}"),
            );
        }

        let result = match hook_timeout {
            Some(timeout_duration) => match timeout(timeout_duration, future).await {
                Ok(result) => result,
                Err(_) => {
                    return Err(HookFailure {
                        hook_name: name,
                        reason: format!("hook timed out after {} ms", timeout_duration.as_millis()),
                    });
                }
            },
            None => future.await,
        };

        match result {
            Ok(()) => Ok(()),
            Err(reason) => Err(HookFailure {
                hook_name: name,
                reason,
            }),
        }
    }

    pub(super) fn log_hook_failure(&self, phase: LifecyclePhase, failure: &HookFailure) {
        if let Some(logger) = &self.logger {
            logger.warn_in(
                MODULE_LIFECYCLE,
                format!(
                    "phase={phase:?} hook={} failed: {}",
                    failure.hook_name, failure.reason
                ),
            );
        }
    }
}
