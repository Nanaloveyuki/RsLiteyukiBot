use super::*;

impl Lifespan {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_logger(logger: Logger) -> Self {
        Self {
            logger: Some(logger),
            ..Self::default()
        }
    }

    pub fn set_logger(&mut self, logger: Logger) {
        self.logger = Some(logger);
    }

    pub fn set_failure_policy(&mut self, policy: LifecycleFailurePolicy) {
        self.failure_policy = policy;
    }

    pub fn set_hook_timeout(&mut self, timeout: Option<Duration>) {
        self.hook_timeout = timeout;
    }

    pub fn on_before_start<F, Fut>(&mut self, name: impl Into<String>, filter: HookFilter, hook: F)
    where
        F: Fn(Arc<LifecycleContext>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.before_start_hooks
            .push(HookRegistration::new(name, filter, hook));
    }

    pub fn on_after_start<F, Fut>(&mut self, name: impl Into<String>, filter: HookFilter, hook: F)
    where
        F: Fn(Arc<LifecycleContext>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.after_start_hooks
            .push(HookRegistration::new(name, filter, hook));
    }

    pub fn on_before_process_shutdown<F, Fut>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>, Arc<str>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.before_process_shutdown_hooks
            .push(ProcessHookRegistration::new(name, filter, hook));
    }

    pub fn on_after_shutdown<F, Fut>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.after_shutdown_hooks
            .push(HookRegistration::new(name, filter, hook));
    }

    pub fn on_before_process_restart<F, Fut>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>, Arc<str>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.before_process_restart_hooks
            .push(ProcessHookRegistration::new(name, filter, hook));
    }

    pub fn on_after_restart<F, Fut>(&mut self, name: impl Into<String>, filter: HookFilter, hook: F)
    where
        F: Fn(Arc<LifecycleContext>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.after_restart_hooks
            .push(HookRegistration::new(name, filter, hook));
    }

    pub fn on_before_shutdown<F, Fut>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.on_before_process_shutdown(name, filter, move |context, _process_name| hook(context));
    }

    pub fn on_before_restart<F, Fut>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.on_before_process_restart(name, filter, move |context, _process_name| hook(context));
    }

    pub fn on_before_start_sync<F>(&mut self, name: impl Into<String>, filter: HookFilter, hook: F)
    where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.on_before_start(name, filter, move |context| {
            std::future::ready(hook(context))
        });
    }

    pub fn on_after_start_sync<F>(&mut self, name: impl Into<String>, filter: HookFilter, hook: F)
    where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.on_after_start(name, filter, move |context| {
            std::future::ready(hook(context))
        });
    }

    pub fn on_before_process_shutdown_sync<F>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>, Arc<str>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.on_before_process_shutdown(name, filter, move |context, process_name| {
            std::future::ready(hook(context, process_name))
        });
    }

    pub fn on_after_shutdown_sync<F>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.on_after_shutdown(name, filter, move |context| {
            std::future::ready(hook(context))
        });
    }

    pub fn on_before_process_restart_sync<F>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>, Arc<str>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.on_before_process_restart(name, filter, move |context, process_name| {
            std::future::ready(hook(context, process_name))
        });
    }

    pub fn on_after_restart_sync<F>(&mut self, name: impl Into<String>, filter: HookFilter, hook: F)
    where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.on_after_restart(name, filter, move |context| {
            std::future::ready(hook(context))
        });
    }

    pub fn on_before_shutdown_sync<F>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.on_before_shutdown(name, filter, move |context| {
            std::future::ready(hook(context))
        });
    }

    pub fn on_before_restart_sync<F>(
        &mut self,
        name: impl Into<String>,
        filter: HookFilter,
        hook: F,
    ) where
        F: Fn(Arc<LifecycleContext>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.on_before_restart(name, filter, move |context| {
            std::future::ready(hook(context))
        });
    }
}

impl HookRegistration {
    fn new<F, Fut>(name: impl Into<String>, filter: HookFilter, hook: F) -> Self
    where
        F: Fn(Arc<LifecycleContext>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let handler: HookHandler = Arc::new(move |context| Box::pin(hook(context)));
        Self {
            name: name.into(),
            filter,
            handler,
        }
    }
}

impl ProcessHookRegistration {
    fn new<F, Fut>(name: impl Into<String>, filter: HookFilter, hook: F) -> Self
    where
        F: Fn(Arc<LifecycleContext>, Arc<str>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let handler: ProcessHookHandler =
            Arc::new(move |context, process_name| Box::pin(hook(context, process_name)));
        Self {
            name: name.into(),
            filter,
            handler,
        }
    }
}
