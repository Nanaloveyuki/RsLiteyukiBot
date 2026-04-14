use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use tokio::task::JoinSet;

use crate::observability::Logger;

const MODULE_LIFECYCLE: &str = "core.lifecycle";
const DEFAULT_PROCESS_NAME: &str = "main";

type HookFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;
type HookHandler = Arc<dyn Fn(Arc<LifecycleContext>) -> HookFuture + Send + Sync + 'static>;
type ProcessHookHandler =
    Arc<dyn Fn(Arc<LifecycleContext>, Arc<str>) -> HookFuture + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePhase {
    BeforeStart,
    AfterStart,
    BeforeProcessShutdown,
    AfterShutdown,
    BeforeProcessRestart,
    AfterRestart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleFailurePolicy {
    FailFast,
    Continue,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuntimeFlavor {
    Cli,
    Web,
    DesktopTauri2,
    Docker,
    Llm,
    Service,
    Custom(String),
}

impl RuntimeFlavor {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cli" => Some(Self::Cli),
            "web" => Some(Self::Web),
            "desktop" | "tauri" | "tauri2" => Some(Self::DesktopTauri2),
            "docker" | "container" => Some(Self::Docker),
            "llm" => Some(Self::Llm),
            "service" => Some(Self::Service),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCapabilities {
    pub llm: bool,
    pub desktop_tauri2: bool,
    pub docker: bool,
    pub cli: bool,
    pub web: bool,
}

impl RuntimeCapabilities {
    pub fn from_flavor(flavor: &RuntimeFlavor) -> Self {
        match flavor {
            RuntimeFlavor::Cli => Self {
                llm: false,
                desktop_tauri2: false,
                docker: false,
                cli: true,
                web: false,
            },
            RuntimeFlavor::Web => Self {
                llm: false,
                desktop_tauri2: false,
                docker: false,
                cli: false,
                web: true,
            },
            RuntimeFlavor::DesktopTauri2 => Self {
                llm: false,
                desktop_tauri2: true,
                docker: false,
                cli: false,
                web: false,
            },
            RuntimeFlavor::Docker => Self {
                llm: false,
                desktop_tauri2: false,
                docker: true,
                cli: false,
                web: false,
            },
            RuntimeFlavor::Llm => Self {
                llm: true,
                desktop_tauri2: false,
                docker: false,
                cli: false,
                web: false,
            },
            RuntimeFlavor::Service => Self {
                llm: false,
                desktop_tauri2: false,
                docker: false,
                cli: false,
                web: false,
            },
            RuntimeFlavor::Custom(_) => Self {
                llm: false,
                desktop_tauri2: false,
                docker: false,
                cli: false,
                web: false,
            },
        }
    }

    pub fn with_env_overrides(mut self) -> Self {
        if let Some(value) = parse_bool_env("LY_CAP_LLM") {
            self.llm = value;
        }
        if let Some(value) = parse_bool_env("LY_CAP_TAURI2") {
            self.desktop_tauri2 = value;
        }
        if let Some(value) = parse_bool_env("LY_CAP_DOCKER") {
            self.docker = value;
        }
        if let Some(value) = parse_bool_env("LY_CAP_CLI") {
            self.cli = value;
        }
        if let Some(value) = parse_bool_env("LY_CAP_WEB") {
            self.web = value;
        }
        self
    }
}

impl Default for RuntimeCapabilities {
    fn default() -> Self {
        Self {
            llm: false,
            desktop_tauri2: false,
            docker: false,
            cli: true,
            web: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LifecycleContext {
    app_name: Arc<str>,
    app_version: Arc<str>,
    runtime_flavor: RuntimeFlavor,
    capabilities: RuntimeCapabilities,
    metadata: Arc<RwLock<HashMap<String, String>>>,
    restart_count: Arc<AtomicU32>,
}

impl LifecycleContext {
    pub fn new_with_capabilities(
        app_name: impl Into<String>,
        app_version: impl Into<String>,
        runtime_flavor: RuntimeFlavor,
        capabilities: RuntimeCapabilities,
    ) -> Self {
        Self {
            app_name: Arc::from(app_name.into()),
            app_version: Arc::from(app_version.into()),
            runtime_flavor,
            capabilities: capabilities.with_env_overrides(),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            restart_count: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn new(
        app_name: impl Into<String>,
        app_version: impl Into<String>,
        runtime_flavor: RuntimeFlavor,
    ) -> Self {
        let capabilities = RuntimeCapabilities::from_flavor(&runtime_flavor);
        Self::new_with_capabilities(app_name, app_version, runtime_flavor, capabilities)
    }

    pub fn from_env(app_name: impl Into<String>, app_version: impl Into<String>) -> Self {
        let runtime_flavor = std::env::var("LY_RUNTIME_FLAVOR")
            .ok()
            .and_then(|raw| RuntimeFlavor::parse(&raw))
            .unwrap_or(RuntimeFlavor::Cli);
        Self::new(app_name, app_version, runtime_flavor)
    }

    pub fn app_name(&self) -> &str {
        self.app_name.as_ref()
    }

    pub fn app_version(&self) -> &str {
        self.app_version.as_ref()
    }

    pub fn runtime_flavor(&self) -> &RuntimeFlavor {
        &self.runtime_flavor
    }

    pub fn capabilities(&self) -> &RuntimeCapabilities {
        &self.capabilities
    }

    pub fn set_meta(&self, key: impl Into<String>, value: impl Into<String>) {
        let mut lock = self
            .metadata
            .write()
            .expect("lifecycle metadata lock should not be poisoned");
        lock.insert(key.into(), value.into());
    }

    pub fn get_meta(&self, key: &str) -> Option<String> {
        let lock = self
            .metadata
            .read()
            .expect("lifecycle metadata lock should not be poisoned");
        lock.get(key).cloned()
    }

    pub fn metadata_snapshot(&self) -> HashMap<String, String> {
        self.metadata
            .read()
            .expect("lifecycle metadata lock should not be poisoned")
            .clone()
    }

    pub fn restart_count(&self) -> u32 {
        self.restart_count.load(Ordering::SeqCst)
    }

    pub fn increment_restart_count(&self) -> u32 {
        self.restart_count.fetch_add(1, Ordering::SeqCst) + 1
    }
}

#[derive(Debug, Clone, Default)]
pub struct HookFilter {
    pub runtime_flavors: Vec<RuntimeFlavor>,
    pub require_llm: bool,
    pub require_tauri2: bool,
    pub require_docker: bool,
    pub require_cli: bool,
    pub require_web: bool,
}

impl HookFilter {
    fn matches(&self, context: &LifecycleContext) -> bool {
        if !self.runtime_flavors.is_empty()
            && !self.runtime_flavors.contains(context.runtime_flavor())
        {
            return false;
        }

        let caps = context.capabilities();
        (!self.require_llm || caps.llm)
            && (!self.require_tauri2 || caps.desktop_tauri2)
            && (!self.require_docker || caps.docker)
            && (!self.require_cli || caps.cli)
            && (!self.require_web || caps.web)
    }
}

#[derive(Debug, Clone)]
pub struct HookFailure {
    pub hook_name: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct LifecycleExecutionError {
    pub phase: LifecyclePhase,
    pub failures: Vec<HookFailure>,
}

impl std::fmt::Display for LifecycleExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lifecycle phase {:?} failed with {} error(s)",
            self.phase,
            self.failures.len()
        )
    }
}

impl std::error::Error for LifecycleExecutionError {}

#[derive(Clone)]
struct HookRegistration {
    name: String,
    filter: HookFilter,
    handler: HookHandler,
}

#[derive(Clone)]
struct ProcessHookRegistration {
    name: String,
    filter: HookFilter,
    handler: ProcessHookHandler,
}

#[derive(Clone)]
pub struct Lifespan {
    before_start_hooks: Vec<HookRegistration>,
    after_start_hooks: Vec<HookRegistration>,
    before_process_shutdown_hooks: Vec<ProcessHookRegistration>,
    after_shutdown_hooks: Vec<HookRegistration>,
    before_process_restart_hooks: Vec<ProcessHookRegistration>,
    after_restart_hooks: Vec<HookRegistration>,
    failure_policy: LifecycleFailurePolicy,
    logger: Option<Logger>,
}

impl Default for Lifespan {
    fn default() -> Self {
        Self {
            before_start_hooks: Vec::new(),
            after_start_hooks: Vec::new(),
            before_process_shutdown_hooks: Vec::new(),
            after_shutdown_hooks: Vec::new(),
            before_process_restart_hooks: Vec::new(),
            after_restart_hooks: Vec::new(),
            failure_policy: LifecycleFailurePolicy::FailFast,
            logger: None,
        }
    }
}

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

    pub async fn before_start(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_phase(
            LifecyclePhase::BeforeStart,
            &self.before_start_hooks,
            context,
        )
        .await
    }

    pub async fn after_start(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_phase(LifecyclePhase::AfterStart, &self.after_start_hooks, context)
            .await
    }

    pub async fn before_process_shutdown(
        &self,
        context: Arc<LifecycleContext>,
        process_name: impl Into<Arc<str>>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_process_phase(
            LifecyclePhase::BeforeProcessShutdown,
            &self.before_process_shutdown_hooks,
            context,
            process_name.into(),
        )
        .await
    }

    pub async fn after_shutdown(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_phase(
            LifecyclePhase::AfterShutdown,
            &self.after_shutdown_hooks,
            context,
        )
        .await
    }

    pub async fn before_process_restart(
        &self,
        context: Arc<LifecycleContext>,
        process_name: impl Into<Arc<str>>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_process_phase(
            LifecyclePhase::BeforeProcessRestart,
            &self.before_process_restart_hooks,
            context,
            process_name.into(),
        )
        .await
    }

    pub async fn after_restart(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.run_phase(
            LifecyclePhase::AfterRestart,
            &self.after_restart_hooks,
            context,
        )
        .await
    }

    pub async fn before_shutdown(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.before_process_shutdown(context, Arc::<str>::from(DEFAULT_PROCESS_NAME))
            .await
    }

    pub async fn before_restart(
        &self,
        context: Arc<LifecycleContext>,
    ) -> Result<(), LifecycleExecutionError> {
        self.before_process_restart(context, Arc::<str>::from(DEFAULT_PROCESS_NAME))
            .await
    }

    async fn run_phase(
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
            let logger = self.logger.clone();

            join_set.spawn(async move {
                if let Some(logger) = &logger {
                    logger.debug_in(
                        MODULE_LIFECYCLE,
                        format!("phase={phase:?} hook={name} start"),
                    );
                }

                match (handler)(hook_context).await {
                    Ok(()) => Ok(()),
                    Err(reason) => Err(HookFailure {
                        hook_name: name,
                        reason,
                    }),
                }
            });
        }

        self.collect_phase_outcome(phase, join_set).await
    }

    async fn run_process_phase(
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
            let logger = self.logger.clone();

            join_set.spawn(async move {
                if let Some(logger) = &logger {
                    logger.debug_in(
                        MODULE_LIFECYCLE,
                        format!(
                            "phase={phase:?} hook={name} process={} start",
                            hook_process_name
                        ),
                    );
                }

                match (handler)(hook_context, hook_process_name).await {
                    Ok(()) => Ok(()),
                    Err(reason) => Err(HookFailure {
                        hook_name: name,
                        reason,
                    }),
                }
            });
        }

        self.collect_phase_outcome(phase, join_set).await
    }

    async fn collect_phase_outcome(
        &self,
        phase: LifecyclePhase,
        mut join_set: JoinSet<Result<(), HookFailure>>,
    ) -> Result<(), LifecycleExecutionError> {
        let mut failures = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(failure)) => {
                    if let Some(logger) = &self.logger {
                        logger.warn_in(
                            MODULE_LIFECYCLE,
                            format!(
                                "phase={phase:?} hook={} failed: {}",
                                failure.hook_name, failure.reason
                            ),
                        );
                    }

                    failures.push(failure);
                    if self.failure_policy == LifecycleFailurePolicy::FailFast {
                        join_set.abort_all();
                        break;
                    }
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

                    if let Some(logger) = &self.logger {
                        logger.warn_in(
                            MODULE_LIFECYCLE,
                            format!(
                                "phase={phase:?} hook={} failed: {}",
                                failure.hook_name, failure.reason
                            ),
                        );
                    }

                    failures.push(failure);
                    if self.failure_policy == LifecycleFailurePolicy::FailFast {
                        join_set.abort_all();
                        break;
                    }
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

fn parse_bool_env(key: &str) -> Option<bool> {
    match std::env::var(key) {
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        },
        Err(_) => None,
    }
}
