use super::*;

impl LiteyukiBot {
    pub async fn start_adapters(&self) -> Result<(), LiteyukiBotError> {
        let handle = self
            .runtime_handle
            .as_ref()
            .ok_or(LiteyukiBotError::NotStarted)?;
        let ingress = handle.ingress_sender();
        let event_seq = Arc::clone(&self.inbound_adapter_event_seq);
        let logger = self.logger.clone();
        let sink = sink_from_fn(move |packet| {
            let ingress = ingress.clone();
            let event_seq = Arc::clone(&event_seq);
            let logger = logger.clone();
            async move {
                let fallback_id = event_seq.fetch_add(1, Ordering::SeqCst);
                let event = packet.into_bot_event(fallback_id);
                let event_id = event.id;
                let event_topic = event.topic.clone();
                if let Err(err) = ingress.send(event).await {
                    logger.warn_in(
                        MODULE_BOT,
                        format!(
                            "adapter ingress dropped event id={} topic={} reason={}",
                            event_id, event_topic, err
                        ),
                    );
                }
            }
        });

        self.adapter_manager
            .start_enabled(sink)
            .await
            .map_err(LiteyukiBotError::Adapter)
    }

    pub async fn stop_adapters(&self) -> Result<(), LiteyukiBotError> {
        self.adapter_manager
            .shutdown_all()
            .await
            .map_err(LiteyukiBotError::Adapter)
    }

    pub async fn reload_adapters<I>(
        &self,
        adapter_configs: I,
        autostart: bool,
    ) -> Result<(), LiteyukiBotError>
    where
        I: IntoIterator<Item = AdapterConfig>,
    {
        self.stop_adapters().await?;
        self.adapter_manager
            .replace_configs(adapter_configs)
            .map_err(LiteyukiBotError::Adapter)?;
        if autostart {
            self.start_adapters().await?;
        }
        Ok(())
    }
}
