use std::sync::Arc;

use tokio::task::JoinSet;

use super::*;

impl Lifespan {
    pub(super) async fn run_phase(
        &self,
        phase: LifecyclePhase,
        hooks: &[HookRegistration],
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        match self.failure_policy {
            LifecycleFailurePolicy::FailFast => {
                self.run_phase_fail_fast(phase, hooks, context).await
            }
            LifecycleFailurePolicy::Continue => {
                self.run_phase_continue(phase, hooks, context).await
            }
        }
    }

    pub(super) async fn run_process_phase(
        &self,
        phase: LifecyclePhase,
        hooks: &[ProcessHookRegistration],
        context: Arc<LifecycleContext>,
        process_name: Arc<str>,
    ) -> Result<(), LifecycleExecutionError> {
        match self.failure_policy {
            LifecycleFailurePolicy::FailFast => {
                self.run_process_phase_fail_fast(phase, hooks, context, process_name)
                    .await
            }
            LifecycleFailurePolicy::Continue => {
                self.run_process_phase_continue(phase, hooks, context, process_name)
                    .await
            }
        }
    }

    async fn run_phase_fail_fast(
        &self,
        phase: LifecyclePhase,
        hooks: &[HookRegistration],
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        for hook in hooks {
            if !hook.filter.matches(context.as_ref()) {
                continue;
            }

            let result = self
                .execute_hook(
                    phase,
                    hook.name.clone(),
                    Arc::clone(&hook.handler),
                    context.clone(),
                )
                .await;
            if let Err(failure) = result {
                self.log_hook_failure(phase, &failure);
                return Err(LifecycleExecutionError {
                    phase,
                    failures: vec![failure],
                });
            }
        }

        Ok(())
    }

    async fn run_phase_continue(
        &self,
        phase: LifecyclePhase,
        hooks: &[HookRegistration],
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        let mut join_set: JoinSet<Result<(), HookFailure>> = JoinSet::new();

        for hook in hooks {
            if !hook.filter.matches(context.as_ref()) {
                continue;
            }

            let name = hook.name.clone();
            let handler = Arc::clone(&hook.handler);
            let hook_context = context.clone();
            let hook_timeout = self.hook_timeout;
            let logger = self.logger.clone();

            join_set.spawn(async move {
                Self::execute_hook_impl(
                    phase,
                    name,
                    "start".to_string(),
                    logger,
                    hook_timeout,
                    (handler)(hook_context),
                )
                .await
            });
        }

        self.collect_continue_phase_outcome(phase, join_set).await
    }

    async fn run_process_phase_fail_fast(
        &self,
        phase: LifecyclePhase,
        hooks: &[ProcessHookRegistration],
        context: Arc<LifecycleContext>,
        process_name: Arc<str>,
    ) -> Result<(), LifecycleExecutionError> {
        for hook in hooks {
            if !hook.filter.matches(context.as_ref()) {
                continue;
            }

            let result = self
                .execute_process_hook(
                    phase,
                    hook.name.clone(),
                    Arc::clone(&hook.handler),
                    context.clone(),
                    Arc::clone(&process_name),
                )
                .await;
            if let Err(failure) = result {
                self.log_hook_failure(phase, &failure);
                return Err(LifecycleExecutionError {
                    phase,
                    failures: vec![failure],
                });
            }
        }

        Ok(())
    }

    async fn run_process_phase_continue(
        &self,
        phase: LifecyclePhase,
        hooks: &[ProcessHookRegistration],
        context: Arc<LifecycleContext>,
        process_name: Arc<str>,
    ) -> Result<(), LifecycleExecutionError> {
        let mut join_set: JoinSet<Result<(), HookFailure>> = JoinSet::new();

        for hook in hooks {
            if !hook.filter.matches(context.as_ref()) {
                continue;
            }

            let name = hook.name.clone();
            let handler = Arc::clone(&hook.handler);
            let hook_context = context.clone();
            let hook_process_name = Arc::clone(&process_name);
            let hook_timeout = self.hook_timeout;
            let logger = self.logger.clone();

            join_set.spawn(async move {
                Self::execute_hook_impl(
                    phase,
                    name,
                    format!("process={} start", hook_process_name),
                    logger,
                    hook_timeout,
                    (handler)(hook_context, hook_process_name),
                )
                .await
            });
        }

        self.collect_continue_phase_outcome(phase, join_set).await
    }

    async fn collect_continue_phase_outcome(
        &self,
        phase: LifecyclePhase,
        mut join_set: JoinSet<Result<(), HookFailure>>,
    ) -> Result<(), LifecycleExecutionError> {
        let mut failures = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(failure)) => {
                    self.log_hook_failure(phase, &failure);
                    failures.push(failure);
                }
                Err(err) => {
                    let reason = if err.is_cancelled() {
                        "hook task cancelled".to_string()
                    } else {
                        format!("hook task join error: {err}")
                    };
                    let failure = HookFailure {
                        hook_name: "<joinset>".to_string(),
                        reason,
                    };

                    self.log_hook_failure(phase, &failure);
                    failures.push(failure);
                }
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(LifecycleExecutionError { phase, failures })
        }
    }
}
