use super::*;

impl LiteyukiBot {
    pub async fn start(&mut self) -> Result<(), LiteyukiBotError> {
        if self.runtime_handle.is_some() {
            return Err(LiteyukiBotError::AlreadyStarted);
        }

        for hook in &self.bootstrap_hooks {
            hook(BotBootstrapContext {
                target: self.target,
                lifecycle: self.lifecycle.clone(),
                channels: self.channels.clone(),
                shared_store: self.shared_store.clone(),
                session_router: self.session_router.clone(),
                plugin_manager: self.plugin_manager.clone(),
                plugin_sdk: self.plugin_sdk.clone(),
                adapter_manager: self.adapter_manager.clone(),
                logger: self.logger.clone(),
            })
            .await
            .map_err(LiteyukiBotError::Bootstrap)?;
        }

        let mut progress = StartProgress::default();

        if let Err(err) = self.lifespan.before_start(self.lifecycle.clone()).await {
            return Err(err.into());
        }
        progress.before_start_completed = true;

        if let Err(err) = self.process_manager.start_all() {
            self.rollback_failed_start(&progress).await;
            return Err(err.into());
        }
        progress.processes_started = true;

        if let Err(err) = self.load_plugins().await {
            self.rollback_failed_start(&progress).await;
            return Err(err);
        }

        let handle = self.runtime.start();
        self.runtime_handle = Some(handle);
        progress.runtime_started = true;

        if let Err(err) = self
            .plugin_manager
            .start_loaded_plugins(self.plugin_context())
            .await
        {
            self.rollback_failed_start(&progress).await;
            return Err(err.into());
        }

        if self.adapter_autostart {
            progress.adapters_may_be_running = true;
            if let Err(err) = self.start_adapters().await {
                self.rollback_failed_start(&progress).await;
                return Err(err);
            }
        }

        if let Err(err) = self.health_check_plugins().await {
            self.rollback_failed_start(&progress).await;
            return Err(err);
        }

        if let Err(err) = self.lifespan.after_start(self.lifecycle.clone()).await {
            self.rollback_failed_start(&progress).await;
            return Err(err.into());
        }

        self.logger.info_in(
            MODULE_BOT,
            format!("bot started on target {:?}", self.target),
        );
        Ok(())
    }

    async fn rollback_failed_start(&mut self, progress: &StartProgress) {
        if progress.adapters_may_be_running
            && let Err(err) = self.adapter_manager.shutdown_all().await
        {
            self.logger.warn_in(
                MODULE_BOT,
                format!("start rollback: adapter shutdown failed: {err}"),
            );
        }

        if progress.before_start_completed
            && (progress.processes_started || progress.runtime_started)
        {
            let process_names = self.collect_shutdown_process_names(progress.runtime_started);
            for process_name in process_names {
                if let Err(err) = self
                    .lifespan
                    .before_process_shutdown(self.lifecycle.clone(), Arc::clone(&process_name))
                    .await
                {
                    self.logger.warn_in(
                        MODULE_BOT,
                        format!(
                            "start rollback: before_process_shutdown hook failed for '{}': {}",
                            process_name, err
                        ),
                    );
                }
            }
        }

        if progress.runtime_started
            && let Some(handle) = self.runtime_handle.take()
        {
            handle.shutdown().await;
        }

        if progress.processes_started
            && let Err(err) = self.process_manager.terminate_all().await
        {
            self.logger.warn_in(
                MODULE_BOT,
                format!("start rollback: process termination failed: {err}"),
            );
        }

        if let Err(err) = self
            .plugin_manager
            .shutdown_loaded_plugins(self.plugin_context())
            .await
        {
            self.logger.warn_in(
                MODULE_BOT,
                format!("start rollback: plugin shutdown failed: {err}"),
            );
        }

        if progress.before_start_completed
            && let Err(err) = self.lifespan.after_shutdown(self.lifecycle.clone()).await
        {
            self.logger.warn_in(
                MODULE_BOT,
                format!("start rollback: after_shutdown hook failed: {err}"),
            );
        }
    }
}
