pub mod bootstrap;
pub mod comm;
pub mod core;
pub mod observability;
pub mod plugin;
pub mod session;

pub use bootstrap::{
    ConfigError, ConfigManager, ConfigSetting, RuntimeSettings, RuntimeSettingsSpec,
};
pub use comm::{Channel, ChannelError, ChannelMessage, ChannelRegistry, SharedStore};
pub use core::{
    BotBootstrapContext, BotEvent, BotHandle, BotRuntime, BotRuntimeConfig,
    DefaultEventTextFormatter, EventTextFormatter, HookFailure, HookFilter, LifecycleContext,
    LifecycleExecutionError, LifecycleFailurePolicy, LifecyclePhase, Lifespan, LiteyukiBot,
    LiteyukiBotBuilder, LiteyukiBotError, ManagedProcessRunner, ManagedProcessSpec, ProcessManager,
    ProcessManagerError, RestartPolicy, RuntimeCapabilities, RuntimeFlavor, RuntimeTarget,
    format_event_text, format_event_with,
};
pub use observability::{LogLevel, LogMode, Logger, LoggerConfig, TimeZone, TimestampFormat};
pub use plugin::{
    LoadedPlugin, LuaRuntimeAdapter, NativeRuntimeAdapter, Plugin, PluginContext, PluginDescriptor,
    PluginFuture, PluginHostApi, PluginHostBridge, PluginLoadError, PluginLoadPlan,
    PluginLoadState, PluginManager, PluginManifest, PluginManifestError, PluginManifestLoader,
    PluginMetadata, PluginRuntimeKind, PluginRuntimeSpec, PluginSdk, PluginSdkError,
    PluginSdkFuture, PluginSdkSpec, PluginType, PythonRuntimeAdapter, RuntimeAdapter,
    RuntimeAdapterRegistry,
};
pub use session::{Matcher, MatcherReport, Rule, SessionDispatchReport, SessionEvent, SessionRouter, SessionScope};
