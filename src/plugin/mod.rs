mod abi;
mod loader;
mod manager;
mod model;
mod sdk;

pub use abi::{
    PluginAbiContract, PluginAbiMethod, PluginCallEnvelope, PluginCallResult, PluginErrorCode,
    PluginHandshakeRequest, PluginHandshakeResponse,
};
pub use loader::{PluginManifest, PluginManifestError, PluginManifestLoader};
pub use manager::{
    LoadedPlugin, Plugin, PluginContext, PluginFuture, PluginLoadError, PluginManager,
};
pub use model::{
    PluginDescriptor, PluginMetadata, PluginRuntimeKind, PluginRuntimeSpec, PluginSdkSpec,
    PluginType,
};
pub use sdk::{
    LuaRuntimeAdapter, NativeRuntimeAdapter, PluginHostApi, PluginHostBridge, PluginLoadPlan,
    PluginLoadState, PluginSdk, PluginSdkError, PluginSdkFuture, PluginTuiCommand,
    PythonRuntimeAdapter, RuntimeAdapter, RuntimeAdapterRegistry,
};
