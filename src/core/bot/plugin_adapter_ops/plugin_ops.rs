use super::*;

impl LiteyukiBot {
    pub async fn health_check_plugins(&self) -> Result<(), LiteyukiBotError> {
        if self.runtime_handle.is_none() {
            return Err(LiteyukiBotError::NotStarted);
        }
        self.plugin_manager
            .health_check_loaded_plugins(self.plugin_context())
            .await
            .map_err(LiteyukiBotError::Plugin)
    }

    pub(in crate::core::bot) async fn load_plugins(&self) -> Result<(), LiteyukiBotError> {
        let pending = self.collect_pending_plugin_ids().await?;
        if pending.is_empty() {
            return Ok(());
        }

        self.plugin_manager
            .load_plugins(pending, self.plugin_context())
            .await
            .map_err(LiteyukiBotError::Plugin)?;
        Ok(())
    }

    pub(in crate::core::bot) async fn sync_plugins_for_current_policy(
        &self,
    ) -> Result<(), LiteyukiBotError> {
        let pending = self.collect_pending_plugin_ids().await?;
        if pending.is_empty() {
            return Ok(());
        }

        self.plugin_manager
            .load_plugins(pending, self.plugin_context())
            .await
            .map_err(LiteyukiBotError::Plugin)?;
        self.plugin_manager
            .start_loaded_plugins(self.plugin_context())
            .await
            .map_err(LiteyukiBotError::Plugin)?;
        self.plugin_manager
            .health_check_loaded_plugins(self.plugin_context())
            .await
            .map_err(LiteyukiBotError::Plugin)?;
        Ok(())
    }

    async fn collect_pending_plugin_ids(&self) -> Result<Vec<String>, LiteyukiBotError> {
        let discovered = self
            .plugin_manager
            .discover_manifest_plugins_in_dirs(self.plugin_dirs.iter())
            .map_err(LiteyukiBotError::Plugin)?;

        let disabled = self
            .disabled_plugin_ids
            .read()
            .expect("disabled plugin ids lock should not be poisoned")
            .clone();
        let mut pending: Vec<String> = Vec::new();
        let mut visited = HashSet::new();
        for id in discovered.into_iter().chain(self.plugin_ids.clone()) {
            if !visited.insert(id.clone()) {
                continue;
            }
            if disabled.contains(id.as_str()) {
                self.logger
                    .info_in(MODULE_BOT, format!("skip disabled plugin '{}'", id));
                continue;
            }
            pending.push(id);
        }

        Ok(pending)
    }

    pub(in crate::core::bot) fn plugin_context(&self) -> PluginContext {
        let host = PluginHostBridge::new(
            self.lifecycle.clone(),
            self.channels.clone(),
            self.shared_store.clone(),
            self.session_router.clone(),
            self.adapter_manager.clone(),
            self.logger.clone(),
        );
        PluginContext {
            target: self.target,
            lifecycle: self.lifecycle.clone(),
            channels: self.channels.clone(),
            shared_store: self.shared_store.clone(),
            session_router: self.session_router.clone(),
            logger: self.logger.clone(),
            sdk: self.plugin_sdk.clone(),
            host,
        }
    }
}
