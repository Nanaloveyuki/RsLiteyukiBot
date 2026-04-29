use super::*;

impl LiteyukiBot {
    pub async fn shutdown(&mut self) -> Result<(), LiteyukiBotError> {
        if self.runtime_handle.is_none() && !self.process_manager.is_running("runtime") {
            return Err(LiteyukiBotError::NotStarted);
        }

        let mut first_error: Option<LiteyukiBotError> = None;
        let process_names = self.collect_shutdown_process_names(self.runtime_handle.is_some());

        self.run_before_shutdown_hooks(&process_names, "shutdown", &mut first_error)
            .await;

        if let Err(err) = self
            .plugin_manager
            .shutdown_loaded_plugins(self.plugin_context())
            .await
        {
            self.record_first_error(
                &mut first_error,
                "shutdown plugins",
                LiteyukiBotError::Plugin(err),
            );
        }

        if let Err(err) = self.adapter_manager.shutdown_all().await {
            self.record_first_error(
                &mut first_error,
                "shutdown adapter manager",
                LiteyukiBotError::Adapter(err),
            );
        }

        if let Err(err) = self.process_manager.terminate_all().await {
            self.record_first_error(
                &mut first_error,
                "shutdown managed processes",
                LiteyukiBotError::Process(err),
            );
        }

        if let Some(handle) = self.runtime_handle.take() {
            handle.shutdown().await;
        }

        if let Err(err) = self.lifespan.after_shutdown(self.lifecycle.clone()).await {
            self.record_first_error(
                &mut first_error,
                "shutdown after_shutdown hook",
                LiteyukiBotError::Lifecycle(err),
            );
        }

        self.logger.info_in(MODULE_BOT, "bot shutdown complete");

        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    pub(super) fn collect_shutdown_process_names(&self, include_runtime: bool) -> Vec<Arc<str>> {
        let mut process_names = self.process_manager.running_process_names();
        if include_runtime && !process_names.iter().any(|name| name.as_ref() == "runtime") {
            process_names.push(Arc::<str>::from("runtime"));
        }
        process_names
    }

    async fn run_before_shutdown_hooks(
        &self,
        process_names: &[Arc<str>],
        stage: &str,
        first_error: &mut Option<LiteyukiBotError>,
    ) {
        for process_name in process_names {
            if let Err(err) = self
                .lifespan
                .before_process_shutdown(self.lifecycle.clone(), Arc::clone(process_name))
                .await
            {
                self.record_first_error(
                    first_error,
                    &format!("{} before_process_shutdown '{}'", stage, process_name),
                    LiteyukiBotError::Lifecycle(err),
                );
            }
        }
    }

    fn record_first_error(
        &self,
        first_error: &mut Option<LiteyukiBotError>,
        phase: &str,
        err: LiteyukiBotError,
    ) {
        self.logger
            .warn_in(MODULE_BOT, format!("{} failed: {}", phase, err));
        if first_error.is_none() {
            *first_error = Some(err);
        }
    }
}
