#[path = "runner/hook_executor.rs"]
mod hook_executor;
#[path = "runner/phase_runner.rs"]
mod phase_runner;

use std::sync::Arc;

use super::*;

impl Lifespan {
    pub async fn before_start(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_phase(
            LifecyclePhase::BeforeStart,
            &self.before_start_hooks,
            context,
        )
        .await
    }

    pub async fn after_start(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_phase(LifecyclePhase::AfterStart, &self.after_start_hooks, context)
            .await
    }

    pub async fn before_process_shutdown(
        &self,
        context: Arc<LifecycleContext>,
        process_name: impl Into<Arc<str>>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_process_phase(
            LifecyclePhase::BeforeProcessShutdown,
            &self.before_process_shutdown_hooks,
            context,
            process_name.into(),
        )
        .await
    }

    pub async fn after_shutdown(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_phase(
            LifecyclePhase::AfterShutdown,
            &self.after_shutdown_hooks,
            context,
        )
        .await
    }

    pub async fn before_process_restart(
        &self,
        context: Arc<LifecycleContext>,
        process_name: impl Into<Arc<str>>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_process_phase(
            LifecyclePhase::BeforeProcessRestart,
            &self.before_process_restart_hooks,
            context,
            process_name.into(),
        )
        .await
    }

    pub async fn after_restart(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_phase(
            LifecyclePhase::AfterRestart,
            &self.after_restart_hooks,
            context,
        )
        .await
    }

    pub async fn before_shutdown(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.before_process_shutdown(context, Arc::<str>::from(DEFAULT_PROCESS_NAME))
            .await
    }

    pub async fn before_restart(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.before_process_restart(context, Arc::<str>::from(DEFAULT_PROCESS_NAME))
            .await
    }
}
