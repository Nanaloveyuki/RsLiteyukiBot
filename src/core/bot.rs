use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::{
    BotEvent, BotHandle, BotRuntime, BotRuntimeConfig, HookFilter, LifecycleContext,
    LifecycleExecutionError, LifecycleFailurePolicy, Lifespan, ManagedProcessSpec, ProcessManager,
    ProcessManagerError, RuntimeTarget,
};
use crate::adapter::{AdapterConfig, AdapterError, AdapterManager, sink_from_fn};
use crate::comm::{ChannelRegistry, SharedStore};
use crate::observability::Logger;
use crate::plugin::{
    Plugin, PluginContext, PluginHostBridge, PluginLoadError, PluginManager, PluginSdk,
};
use crate::session::{Rule, SessionEvent, SessionRouter};

const MODULE_BOT: &str = "core.bot";

type EventFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type EventHandler = Arc<dyn Fn(BotEvent, Logger) -> EventFuture + Send + Sync + 'static>;
type BootstrapFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;
type BootstrapHook = Arc<dyn Fn(BotBootstrapContext) -> BootstrapFuture + Send + Sync + 'static>;

#[derive(Debug)]
pub enum LiteyukiBotError {
    AlreadyStarted,
    NotStarted,
    Lifecycle(LifecycleExecutionError),
    Process(ProcessManagerError),
    Plugin(PluginLoadError),
    Adapter(AdapterError),
    Bootstrap(String),
    Send(String),
}

impl std::fmt::Display for LiteyukiBotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyStarted => f.write_str("bot runtime is already started"),
            Self::NotStarted => f.write_str("bot runtime is not started"),
            Self::Lifecycle(err) => write!(f, "lifecycle error: {err}"),
            Self::Process(err) => write!(f, "process manager error: {err}"),
            Self::Plugin(err) => write!(f, "plugin error: {err}"),
            Self::Adapter(err) => write!(f, "adapter error: {err}"),
            Self::Bootstrap(err) => write!(f, "bootstrap hook failed: {err}"),
            Self::Send(err) => write!(f, "send event failed: {err}"),
        }
    }
}

impl std::error::Error for LiteyukiBotError {}

impl From<LifecycleExecutionError> for LiteyukiBotError {
    fn from(value: LifecycleExecutionError) -> Self {
        Self::Lifecycle(value)
    }
}

impl From<ProcessManagerError> for LiteyukiBotError {
    fn from(value: ProcessManagerError) -> Self {
        Self::Process(value)
    }
}

impl From<PluginLoadError> for LiteyukiBotError {
    fn from(value: PluginLoadError) -> Self {
        Self::Plugin(value)
    }
}

impl From<AdapterError> for LiteyukiBotError {
    fn from(value: AdapterError) -> Self {
        Self::Adapter(value)
    }
}

#[derive(Clone)]
pub struct BotBootstrapContext {
    pub target: RuntimeTarget,
    pub lifecycle: Arc<LifecycleContext>,
    pub channels: ChannelRegistry,
    pub shared_store: SharedStore,
    pub session_router: SessionRouter,
    pub plugin_manager: PluginManager,
    pub plugin_sdk: PluginSdk,
    pub adapter_manager: AdapterManager,
    pub logger: Logger,
}

pub struct LiteyukiBotBuilder {
    app_name: String,
    app_version: String,
    target: RuntimeTarget,
    runtime_config: BotRuntimeConfig,
    lifecycle_failure_policy: LifecycleFailurePolicy,
    event_handler: Option<EventHandler>,
    plugin_ids: Vec<String>,
    plugin_dirs: Vec<PathBuf>,
    plugin_sdk: Option<PluginSdk>,
    adapter_configs: Vec<AdapterConfig>,
    adapter_autostart: bool,
}

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
        let base_logger = Logger::with_config(runtime_config.logger.clone());
        let session_router = SessionRouter::with_logger(base_logger.clone());
        let plugin_manager = PluginManager::with_logger(base_logger.clone());
        let adapter_manager = AdapterManager::with_logger(base_logger.clone());
        let plugin_sdk = self.plugin_sdk.unwrap_or_default();
        let custom_handler = self.event_handler.clone();
        let runtime_router = session_router.clone();

        let runtime = BotRuntime::with_handler(runtime_config, move |event, logger| {
            let runtime_router = runtime_router.clone();
            let custom_handler = custom_handler.clone();
            async move {
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
                logger.warn_in(
                    MODULE_BOT,
                    format!("skip adapter registration: {}", err),
                );
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
            adapter_autostart: self.adapter_autostart,
            logger,
            bootstrap_hooks: Vec::new(),
            inbound_adapter_event_seq: Arc::new(AtomicU64::new(10_000_000)),
        }
    }
}

pub struct LiteyukiBot {
    target: RuntimeTarget,
    runtime: BotRuntime,
    runtime_handle: Option<BotHandle>,
    lifecycle: Arc<LifecycleContext>,
    lifespan: Lifespan,
    process_manager: ProcessManager,
    channels: ChannelRegistry,
    shared_store: SharedStore,
    session_router: SessionRouter,
    plugin_manager: PluginManager,
    plugin_sdk: PluginSdk,
    adapter_manager: AdapterManager,
    plugin_ids: Vec<String>,
    plugin_dirs: Vec<PathBuf>,
    adapter_autostart: bool,
    logger: Logger,
    bootstrap_hooks: Vec<BootstrapHook>,
    inbound_adapter_event_seq: Arc<AtomicU64>,
}

impl LiteyukiBot {
    pub fn builder(
        app_name: impl Into<String>,
        app_version: impl Into<String>,
    ) -> LiteyukiBotBuilder {
        LiteyukiBotBuilder::new(app_name, app_version)
    }

    pub fn target(&self) -> RuntimeTarget {
        self.target
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn channels(&self) -> &ChannelRegistry {
        &self.channels
    }

    pub fn shared_store(&self) -> &SharedStore {
        &self.shared_store
    }

    pub fn session_router(&self) -> &SessionRouter {
        &self.session_router
    }

    pub fn plugin_manager(&self) -> &PluginManager {
        &self.plugin_manager
    }

    pub fn plugin_sdk(&self) -> &PluginSdk {
        &self.plugin_sdk
    }

    pub fn adapter_manager(&self) -> &AdapterManager {
        &self.adapter_manager
    }

    pub fn lifecycle_context(&self) -> Arc<LifecycleContext> {
        self.lifecycle.clone()
    }

    pub fn lifespan(&self) -> &Lifespan {
        &self.lifespan
    }

    pub fn lifespan_mut(&mut self) -> &mut Lifespan {
        &mut self.lifespan
    }

    pub fn process_manager(&self) -> &ProcessManager {
        &self.process_manager
    }

    pub fn register_process<F, Fut>(
        &self,
        name: impl Into<String>,
        spec: ManagedProcessSpec,
        runner: F,
    ) -> Result<(), ProcessManagerError>
    where
        F: Fn(tokio::sync::watch::Receiver<bool>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.process_manager.register(name, spec, runner)
    }

    pub fn register_plugin<P: Plugin + 'static>(&self, plugin: P) -> Result<(), PluginLoadError> {
        self.plugin_manager.register_plugin(plugin)
    }

    pub fn register_adapter(&self, config: AdapterConfig) -> Result<(), AdapterError> {
        self.adapter_manager.register(config)
    }

    pub fn on_message<F, Fut>(
        &self,
        name: impl Into<String>,
        rule: Rule,
        priority: i32,
        block: bool,
        handler: F,
    ) where
        F: Fn(Arc<SessionEvent>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.session_router
            .on_message(name, rule, priority, block, handler);
    }

    pub fn on_keywords<F, Fut>(
        &self,
        name: impl Into<String>,
        keywords: impl IntoIterator<Item = impl Into<String>>,
        priority: i32,
        block: bool,
        handler: F,
    ) where
        F: Fn(Arc<SessionEvent>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.session_router
            .on_keywords(name, keywords, priority, block, handler);
    }

    pub fn on_before_start_sync<F>(&mut self, name: impl Into<String>, filter: HookFilter, hook: F)
    where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.lifespan.on_before_start_sync(name, filter, hook);
    }

    pub fn on_after_start_sync<F>(&mut self, name: impl Into<String>, filter: HookFilter, hook: F)
    where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.lifespan.on_after_start_sync(name, filter, hook);
    }

    pub fn on_before_process_shutdown_sync<F>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>, Arc<str>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.lifespan
            .on_before_process_shutdown_sync(name, filter, hook);
    }

    pub fn on_after_shutdown_sync<F>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.lifespan.on_after_shutdown_sync(name, filter, hook);
    }

    pub fn add_bootstrap_hook<F, Fut>(&mut self, hook: F)
    where
        F: Fn(BotBootstrapContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.bootstrap_hooks
            .push(Arc::new(move |context| Box::pin(hook(context))));
    }

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

        self.lifespan.before_start(self.lifecycle.clone()).await?;
        self.process_manager.start_all()?;

        self.load_plugins().await?;

        let handle = self.runtime.start();
        self.runtime_handle = Some(handle);

        if self.adapter_autostart {
            self.start_adapters().await?;
        }

        self.lifespan.after_start(self.lifecycle.clone()).await?;
        self.logger.info_in(
            MODULE_BOT,
            format!("bot started on target {:?}", self.target),
        );
        Ok(())
    }

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

    pub async fn shutdown(&mut self) -> Result<(), LiteyukiBotError> {
        if self.runtime_handle.is_none() && !self.process_manager.is_running("runtime") {
            return Err(LiteyukiBotError::NotStarted);
        }

        let mut first_error: Option<LiteyukiBotError> = None;

        if let Err(err) = self
            .lifespan
            .before_process_shutdown(self.lifecycle.clone(), Arc::<str>::from("runtime"))
            .await
        {
            first_error = Some(err.into());
        }

        if let Err(err) = self.adapter_manager.shutdown_all().await
            && first_error.is_none()
        {
            first_error = Some(err.into());
        }

        if let Err(err) = self.process_manager.terminate_all().await
            && first_error.is_none()
        {
            first_error = Some(err.into());
        }

        if let Some(handle) = self.runtime_handle.take() {
            handle.shutdown().await;
        }

        if let Err(err) = self.lifespan.after_shutdown(self.lifecycle.clone()).await
            && first_error.is_none()
        {
            first_error = Some(err.into());
        }

        self.logger.info_in(MODULE_BOT, "bot shutdown complete");

        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    async fn load_plugins(&self) -> Result<(), LiteyukiBotError> {
        let discovered = self
            .plugin_manager
            .discover_manifest_plugins_in_dirs(self.plugin_dirs.iter())
            .map_err(LiteyukiBotError::Plugin)?;

        let mut pending: Vec<String> = Vec::new();
        let mut visited = HashSet::new();
        for id in discovered.into_iter().chain(self.plugin_ids.clone()) {
            if visited.insert(id.clone()) {
                pending.push(id);
            }
        }

        if pending.is_empty() {
            return Ok(());
        }

        self.plugin_manager
            .load_plugins(pending, self.plugin_context())
            .await
            .map_err(LiteyukiBotError::Plugin)?;
        Ok(())
    }

    pub async fn start_adapters(&self) -> Result<(), LiteyukiBotError> {
        let handle = self
            .runtime_handle
            .as_ref()
            .ok_or(LiteyukiBotError::NotStarted)?;
        let ingress = handle.ingress_sender();
        let event_seq = Arc::clone(&self.inbound_adapter_event_seq);
        let sink = sink_from_fn(move |packet| {
            let ingress = ingress.clone();
            let event_seq = Arc::clone(&event_seq);
            async move {
                let fallback_id = event_seq.fetch_add(1, Ordering::SeqCst);
                let event = packet.into_bot_event(fallback_id);
                let _ = ingress.send(event).await;
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

    fn plugin_context(&self) -> PluginContext {
        let host = PluginHostBridge::new(
            self.lifecycle.clone(),
            self.channels.clone(),
            self.shared_store.clone(),
            self.session_router.clone(),
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
