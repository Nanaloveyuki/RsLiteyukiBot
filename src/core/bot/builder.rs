use super::*;

impl LiteyukiBotBuilder {
    pub fn new(app_name: impl Into<String>, app_version: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            app_version: app_version.into(),
            target: RuntimeTarget::default(),
            runtime_config: BotRuntimeConfig::default(),
            lifecycle_failure_policy: LifecycleFailurePolicy::FailFast,
            event_handler: None,
            plugin_ids: Vec::new(),
            plugin_dirs: Vec::new(),
            plugin_sdk: None,
            adapter_configs: Vec::new(),
            adapter_autostart: false,
        }
    }

    pub fn with_target(mut self, target: RuntimeTarget) -> Self {
        self.target = target;
        self
    }

    pub fn with_runtime_config(mut self, config: BotRuntimeConfig) -> Self {
        self.runtime_config = config;
        self
    }

    pub fn with_lifecycle_failure_policy(mut self, policy: LifecycleFailurePolicy) -> Self {
        self.lifecycle_failure_policy = policy;
        self
    }

    pub fn with_event_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(BotEvent, Logger) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.event_handler = Some(Arc::new(move |event, logger| {
            Box::pin(handler(event, logger))
        }));
        self
    }

    pub fn with_plugin_ids<I, S>(mut self, plugin_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.plugin_ids = plugin_ids.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_plugin_dirs<I, P>(mut self, plugin_dirs: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.plugin_dirs = plugin_dirs.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_plugin_sdk(mut self, sdk: PluginSdk) -> Self {
        self.plugin_sdk = Some(sdk);
        self
    }

    pub fn with_adapter_configs<I>(mut self, adapter_configs: I) -> Self
    where
        I: IntoIterator<Item = AdapterConfig>,
    {
        self.adapter_configs = adapter_configs.into_iter().collect();
        self
    }

    pub fn with_adapter_autostart(mut self, enabled: bool) -> Self {
        self.adapter_autostart = enabled;
        self
    }

    pub fn build(self) -> LiteyukiBot {
        let runtime_config = self.target.tune_runtime_config(self.runtime_config);
        let adapter_parallelism = runtime_config.worker_count.max(1);
        let base_logger = Logger::with_config(runtime_config.logger.clone());
        let session_router = SessionRouter::with_logger(base_logger.clone());
        let plugin_manager = PluginManager::with_logger(base_logger.clone());
        let adapter_manager =
            AdapterManager::with_logger(base_logger.clone()).with_parallelism(adapter_parallelism);
        let plugin_sdk = self.plugin_sdk.unwrap_or_default();
        let runtime_plugin_sdk = plugin_sdk.clone();
        let custom_handler = self.event_handler.clone();
        let runtime_router = session_router.clone();

        let runtime = BotRuntime::with_handler(runtime_config, move |event, logger| {
            let runtime_router = runtime_router.clone();
            let custom_handler = custom_handler.clone();
            let runtime_plugin_sdk = runtime_plugin_sdk.clone();
            async move {
                runtime_plugin_sdk.dispatch_event(&event, &logger);
                let session_event = SessionEvent::from_bot_event(&event);
                let dispatch_report = runtime_router.dispatch(session_event).await;
                if !dispatch_report.errors.is_empty() {
                    logger.warn_in(
                        MODULE_BOT,
                        format!(
                            "session dispatch errors={}, topic={}",
                            dispatch_report.errors.len(),
                            event.topic
                        ),
                    );
                }
                if let Some(handler) = custom_handler {
                    handler(event, logger).await;
                }
            }
        });

        let logger = runtime.logger();
        let mut lifespan = Lifespan::with_logger(logger.clone());
        lifespan.set_failure_policy(self.lifecycle_failure_policy);
        let mut process_manager = ProcessManager::with_logger(logger.clone());
        process_manager.set_logger(logger.clone());

        let lifecycle = Arc::new(LifecycleContext::new_with_capabilities(
            self.app_name,
            self.app_version,
            self.target.runtime_flavor(),
            self.target.capabilities(),
        ));

        let channels = ChannelRegistry::default();
        let shared_store = SharedStore::new(channels.clone());

        for config in self.adapter_configs {
            if let Err(err) = adapter_manager.register(config) {
                logger.warn_in(MODULE_BOT, format!("skip adapter registration: {}", err));
            }
        }

        LiteyukiBot {
            target: self.target,
            runtime,
            runtime_handle: None,
            lifecycle,
            lifespan,
            process_manager,
            channels,
            shared_store,
            session_router,
            plugin_manager,
            plugin_sdk,
            adapter_manager,
            plugin_ids: self.plugin_ids,
            plugin_dirs: self.plugin_dirs,
            disabled_plugin_ids: Arc::new(RwLock::new(HashSet::new())),
            adapter_autostart: self.adapter_autostart,
            logger,
            bootstrap_hooks: Vec::new(),
            inbound_adapter_event_seq: Arc::new(AtomicU64::new(10_000_000)),
        }
    }
}
