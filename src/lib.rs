extern crate self as liteyukibot_core;

pub mod adapter;
pub mod app_host;
pub mod bootstrap;
pub mod comm;
pub mod core;
pub mod flow_local_agent;
pub(crate) mod hardcode_data;
pub mod llm;
pub mod observability;
pub mod plugin;
pub(crate) mod runtime_support;
pub mod session;
#[doc(hidden)]
pub mod test_support;
pub(crate) mod utils;
pub mod web;
pub mod web_host;
pub mod web_ui;

// 外部调用
#[allow(dead_code)]
mod app_config;
// 外部调用
#[allow(dead_code)]
mod command_registry;
// 外部调用
#[allow(dead_code)]
mod config_edit;
mod external_commands;
// 外部调用
#[allow(dead_code)]
mod i18n;
// 外部调用
#[allow(dead_code)]
mod onebot_support;
// 外部调用
#[allow(dead_code)]
mod superuser;
// 外部调用
#[allow(dead_code, unused_imports)]
mod tui;

pub use adapter::{
    AdapterConfig, AdapterEndpoint, AdapterError, AdapterManager, AdapterPacket, AdapterRoute,
    AdapterSink, AdapterSinkFuture, AdapterTransport, HttpMethod, HttpTransportClient,
    ManagedAdapterSink, ManagedAdapterSinkFuture, SseEvent, SseParser, SseTransportClient,
    WebSocketAdapterHandle, decode_sse_event, encode_sse_event, sink_from_fn,
    start_forward_adapter, start_reverse_adapter,
};
pub use app_config::{
    DesktopCloseBehavior, persist_desktop_close_to_tray_preference, resolve_desktop_close_behavior,
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
    LlmClientError, LlmCompletion, LlmEventSink, LlmExecutedToolCall, LlmFunctionTool,
    LlmPromptPreview, LlmPromptProfile, LlmPromptStore, LlmStreamEvent, LlmToolOutput,
    OpenAiResponsesClient, OpenAiRuntimeConfig, build_prompt_preview, compose_user_prompt,
};
pub use observability::{
    BufferedLogEntry, LogLevel, LogMode, Logger, LoggerConfig, TimeZone, TimestampFormat,
    emit_console_log, recent_buffered_logs,
};
pub use plugin::discover_plugin_manifests_in_dirs;
pub use plugin::{
    ExternalRuntimeAdapter, LoadedPlugin, LuaRuntimeAdapter, NativeRuntimeAdapter, Plugin,
    PluginAbiContract, PluginAbiMethod, PluginCallEnvelope, PluginCallResult,
    PluginCapabilitySnapshot, PluginCapabilitySource, PluginCatalogEntry, PluginContext,
    PluginDescriptor, PluginErrorCode, PluginExecutionRecord, PluginFuture, PluginHandshakeRequest,
    PluginHandshakeResponse, PluginHostApi, PluginHostBridge, PluginLoadError, PluginLoadPlan,
    PluginLoadState, PluginManager, PluginManifest, PluginManifestError, PluginManifestLoader,
    PluginMetadata, PluginRegisteredCronJob, PluginRegisteredTask, PluginRegisteredTool,
    PluginRegisteredWebApi, PluginRuntimeDiagnostics, PluginRuntimeKind, PluginRuntimeSpec,
    PluginScopedCommand, PluginSdk, PluginSdkError, PluginSdkFuture, PluginSdkSpec,
    PluginToolResult, PluginTuiCommand, PluginType, PluginWebApiRequest, PluginWebApiResponse,
    PythonRuntimeAdapter, RuntimeAdapter, RuntimeAdapterRegistry,
};
pub use session::{
    Matcher, MatcherReport, Rule, SessionDispatchReport, SessionEvent, SessionRouter, SessionScope,
};
