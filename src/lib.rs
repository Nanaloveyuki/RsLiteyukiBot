pub mod adapter;
pub mod bootstrap;
pub mod comm;
pub mod core;
pub mod llm;
pub mod observability;
pub mod plugin;
pub mod session;

#[allow(dead_code)]
mod command_registry;
#[allow(dead_code)]
mod i18n;
#[allow(dead_code)]
mod onebot_support;

pub use adapter::{
    AdapterConfig, AdapterEndpoint, AdapterError, AdapterManager, AdapterPacket, AdapterRoute,
    AdapterSink, AdapterSinkFuture, AdapterTransport, HttpMethod, HttpTransportClient,
    ManagedAdapterSink, ManagedAdapterSinkFuture, SseEvent, SseParser, SseTransportClient,
    WebSocketAdapterHandle, decode_sse_event, encode_sse_event, sink_from_fn,
    start_forward_adapter, start_reverse_adapter,
};
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
pub use llm::{
    LlmClientError, LlmPromptPreview, LlmPromptProfile, LlmPromptStore, OpenAiResponsesClient,
    OpenAiRuntimeConfig, build_prompt_preview, compose_user_prompt,
};
pub use observability::{LogLevel, LogMode, Logger, LoggerConfig, TimeZone, TimestampFormat};
pub use plugin::{
    LoadedPlugin, LuaRuntimeAdapter, NativeRuntimeAdapter, Plugin, PluginAbiContract,
    PluginAbiMethod, PluginCallEnvelope, PluginCallResult, PluginCatalogEntry, PluginContext,
    PluginDescriptor, PluginErrorCode, PluginFuture, PluginHandshakeRequest,
    PluginHandshakeResponse, PluginHostApi, PluginHostBridge, PluginLoadError, PluginLoadPlan,
    PluginLoadState, PluginManager, PluginManifest, PluginManifestError, PluginManifestLoader,
    PluginMetadata, PluginRuntimeKind, PluginRuntimeSpec, PluginScopedCommand, PluginSdk,
    PluginSdkError, PluginSdkFuture, PluginSdkSpec, PluginTuiCommand, PluginType,
    PythonRuntimeAdapter, RuntimeAdapter, RuntimeAdapterRegistry,
};
pub use session::{
    Matcher, MatcherReport, Rule, SessionDispatchReport, SessionEvent, SessionRouter, SessionScope,
};
