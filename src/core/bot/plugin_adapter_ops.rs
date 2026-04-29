#[path = "plugin_adapter_ops/adapter_ops.rs"]
mod adapter_ops;
#[path = "plugin_adapter_ops/plugin_ops.rs"]
mod plugin_ops;

use super::*;

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

    pub fn plugin_dirs(&self) -> &[PathBuf] {
        self.plugin_dirs.as_slice()
    }

    pub fn plugin_sdk(&self) -> &PluginSdk {
        &self.plugin_sdk
    }

    pub fn disabled_plugin_ids(&self) -> Vec<String> {
        let mut entries = self
            .disabled_plugin_ids
            .read()
            .expect("disabled plugin ids lock should not be poisoned")
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }

    pub fn set_disabled_plugin_ids<I, S>(&self, ids: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let next = ids
            .into_iter()
            .map(Into::into)
            .map(|id| id.trim().to_ascii_lowercase())
            .filter(|id| !id.is_empty())
            .collect::<HashSet<_>>();
        let mut lock = self
            .disabled_plugin_ids
            .write()
            .expect("disabled plugin ids lock should not be poisoned");
        *lock = next;
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
}
