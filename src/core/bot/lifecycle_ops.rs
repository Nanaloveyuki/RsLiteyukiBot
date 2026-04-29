#[path = "lifecycle_ops/shutdown.rs"]
mod shutdown;
#[path = "lifecycle_ops/startup.rs"]
mod startup;

use super::*;

impl LiteyukiBot {
    pub async fn send(&self, event: BotEvent) -> Result<(), LiteyukiBotError> {
        let handle = self
            .runtime_handle
            .as_ref()
            .ok_or(LiteyukiBotError::NotStarted)?;
        handle
            .send(event)
            .await
            .map_err(|err| LiteyukiBotError::Send(err.to_string()))
    }

    pub async fn restart_process(&self, name: &str) -> Result<(), LiteyukiBotError> {
        let process_name = Arc::<str>::from(name.to_string());
        self.lifespan
            .before_process_restart(self.lifecycle.clone(), Arc::clone(&process_name))
            .await?;
        self.process_manager.restart(name).await?;
        self.lifecycle.increment_restart_count();
        self.lifespan.after_restart(self.lifecycle.clone()).await?;
        Ok(())
    }

    pub async fn restart_runtime(&mut self) -> Result<(), LiteyukiBotError> {
        if self.runtime_handle.is_none() {
            return Err(LiteyukiBotError::NotStarted);
        }
        let process_name = Arc::<str>::from("runtime");
        self.lifespan
            .before_process_restart(self.lifecycle.clone(), process_name)
            .await?;
        if let Some(handle) = self.runtime_handle.take() {
            handle.shutdown().await;
        }
        self.runtime_handle = Some(self.runtime.start());
        self.lifecycle.increment_restart_count();
        self.lifespan.after_restart(self.lifecycle.clone()).await?;
        Ok(())
    }

    pub async fn reload_plugins<I, S>(&self, disabled_ids: I) -> Result<(), LiteyukiBotError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if self.runtime_handle.is_none() {
            return Err(LiteyukiBotError::NotStarted);
        }

        let previous_disabled = self.disabled_plugin_ids();
        let next_disabled = disabled_ids.into_iter().map(Into::into).collect::<Vec<_>>();
        self.set_disabled_plugin_ids(next_disabled.clone());

        if let Err(err) = self
            .plugin_manager
            .shutdown_loaded_plugins(self.plugin_context())
            .await
        {
            self.logger.warn_in(
                MODULE_BOT,
                format!("plugin reload: shutdown existing plugins reported: {err}"),
            );
        }

        match self.sync_plugins_for_current_policy().await {
            Ok(()) => {
                self.logger.info_in(
                    MODULE_BOT,
                    format!(
                        "plugin reload applied (disabled={})",
                        self.disabled_plugin_ids().len()
                    ),
                );
                Ok(())
            }
            Err(err) => {
                self.logger.warn_in(
                    MODULE_BOT,
                    format!("plugin reload failed, attempting rollback: {err}"),
                );
                if let Err(shutdown_err) = self
                    .plugin_manager
                    .shutdown_loaded_plugins(self.plugin_context())
                    .await
                {
                    self.logger.warn_in(
                        MODULE_BOT,
                        format!("plugin reload rollback shutdown reported: {shutdown_err}"),
                    );
                }
                self.set_disabled_plugin_ids(previous_disabled.clone());
                if let Err(rollback_err) = self.sync_plugins_for_current_policy().await {
                    self.logger.error_in(
                        MODULE_BOT,
                        format!("plugin reload rollback failed: {rollback_err}"),
                    );
                }
                Err(err)
            }
        }
    }
}
