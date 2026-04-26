mod abi;
mod loader;
mod manager;
mod model;
mod sdk;
pub(crate) mod source_adapter;

pub use abi::{
    PluginAbiContract, PluginAbiMethod, PluginCallEnvelope, PluginCallResult, PluginErrorCode,
    PluginHandshakeRequest, PluginHandshakeResponse,
};
pub use loader::{PluginManifest, PluginManifestError, PluginManifestLoader};
pub use manager::{
    LoadedPlugin, Plugin, PluginCatalogEntry, PluginContext, PluginFuture, PluginLoadError,
    PluginManager,
};
pub use model::{
    PluginCapabilitySnapshot, PluginCapabilitySource, PluginCommandDescriptor, PluginDescriptor,
    PluginExecutionRecord, PluginMetadata, PluginRegisteredCronJob, PluginRegisteredTask,
    PluginRegisteredTool, PluginRegisteredWebApi, PluginRuntimeDiagnostics, PluginRuntimeKind,
    PluginRuntimeSpec, PluginSdkSpec, PluginToolResult, PluginType,
};
pub use sdk::{
    ExternalRuntimeAdapter, LuaRuntimeAdapter, NativeRuntimeAdapter, PluginHostApi,
    PluginHostBridge, PluginLoadPlan, PluginLoadState, PluginScopedCommand, PluginSdk,
    PluginSdkError, PluginSdkFuture, PluginTuiCommand, PluginWebApiRequest, PluginWebApiResponse,
    PythonRuntimeAdapter, RuntimeAdapter, RuntimeAdapterRegistry,
};
pub use source_adapter::discover_plugin_manifests_in_dirs;
