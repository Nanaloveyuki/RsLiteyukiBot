#[path = "bot/builder.rs"]
mod builder;
#[path = "bot/lifecycle_ops.rs"]
mod lifecycle_ops;
#[path = "bot/plugin_adapter_ops.rs"]
mod plugin_adapter_ops;

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

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

#[derive(Debug, Default)]
struct StartProgress {
    before_start_completed: bool,
    processes_started: bool,
    runtime_started: bool,
    adapters_may_be_running: bool,
}

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
    disabled_plugin_ids: Arc<RwLock<HashSet<String>>>,
    adapter_autostart: bool,
    logger: Logger,
    bootstrap_hooks: Vec<BootstrapHook>,
    inbound_adapter_event_seq: Arc<AtomicU64>,
}
