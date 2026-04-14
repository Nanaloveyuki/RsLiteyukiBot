use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::comm::{ChannelMessage, ChannelRegistry, SharedStore};
use crate::core::LifecycleContext;
use crate::observability::Logger;
use crate::session::SessionRouter;

use super::{PluginDescriptor, PluginRuntimeKind};

pub type PluginSdkFuture<T> = Pin<Box<dyn Future<Output = Result<T, PluginSdkError>> + Send>>;

#[derive(Debug, Clone)]
pub enum PluginSdkError {
    UnsupportedRuntime {
        kind: PluginRuntimeKind,
        reason: String,
    },
    Host(String),
}

impl std::fmt::Display for PluginSdkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedRuntime { kind, reason } => {
                write!(f, "unsupported runtime {:?}: {}", kind, reason)
            }
            Self::Host(reason) => write!(f, "plugin host error: {}", reason),
        }
    }
}

impl std::error::Error for PluginSdkError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginLoadState {
    Ready,
    Deferred,
}

#[derive(Debug, Clone)]
pub struct PluginLoadPlan {
    pub runtime_kind: PluginRuntimeKind,
    pub state: PluginLoadState,
    pub reason: Option<String>,
}

impl PluginLoadPlan {
    pub fn ready(runtime_kind: PluginRuntimeKind) -> Self {
        Self {
            runtime_kind,
            state: PluginLoadState::Ready,
            reason: None,
        }
    }

    pub fn deferred(runtime_kind: PluginRuntimeKind, reason: impl Into<String>) -> Self {
        Self {
            runtime_kind,
            state: PluginLoadState::Deferred,
            reason: Some(reason.into()),
        }
    }
}

pub trait PluginHostApi: Send + Sync {
    fn log(&self, message: String) -> PluginSdkFuture<()>;
    fn publish(
        &self,
        channel_name: String,
        topic: String,
        payload: Value,
    ) -> PluginSdkFuture<()>;
    fn kv_get(&self, key: String) -> PluginSdkFuture<Option<Value>>;
    fn kv_set(&self, key: String, value: Value) -> PluginSdkFuture<()>;
}

#[derive(Clone)]
pub struct PluginHostBridge {
    lifecycle: Arc<LifecycleContext>,
    channels: ChannelRegistry,
    shared_store: SharedStore,
    session_router: SessionRouter,
    logger: Logger,
}

impl PluginHostBridge {
    pub fn new(
        lifecycle: Arc<LifecycleContext>,
        channels: ChannelRegistry,
        shared_store: SharedStore,
        session_router: SessionRouter,
        logger: Logger,
    ) -> Self {
        Self {
            lifecycle,
            channels,
            shared_store,
            session_router,
            logger,
        }
    }

    pub fn lifecycle(&self) -> Arc<LifecycleContext> {
        self.lifecycle.clone()
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
}

impl PluginHostApi for PluginHostBridge {
    fn log(&self, message: String) -> PluginSdkFuture<()> {
        let logger = self.logger.clone();
        Box::pin(async move {
            logger.info_in("plugin.host", message);
            Ok(())
        })
    }

    fn publish(
        &self,
        channel_name: String,
        topic: String,
        payload: Value,
    ) -> PluginSdkFuture<()> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move {
            let message = ChannelMessage::new(topic, payload, Some("plugin-sdk"));
            shared_store
                .publish(&channel_name, message)
                .map_err(|err| PluginSdkError::Host(err.to_string()))?;
            Ok(())
        })
    }

    fn kv_get(&self, key: String) -> PluginSdkFuture<Option<Value>> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move { Ok(shared_store.get(&key)) })
    }

    fn kv_set(&self, key: String, value: Value) -> PluginSdkFuture<()> {
        let shared_store = self.shared_store.clone();
        Box::pin(async move {
            shared_store.set(key, value);
            Ok(())
        })
    }
}

pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> PluginRuntimeKind;
    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan>;
}

#[derive(Clone, Default)]
pub struct RuntimeAdapterRegistry {
    adapters: Vec<Arc<dyn RuntimeAdapter>>,
}

impl RuntimeAdapterRegistry {
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        registry.register(NativeRuntimeAdapter);
        registry.register(PythonRuntimeAdapter);
        registry.register(LuaRuntimeAdapter);
        registry
    }

    pub fn register<A: RuntimeAdapter + 'static>(&mut self, adapter: A) {
        self.adapters.push(Arc::new(adapter));
    }

    pub fn find(&self, kind: PluginRuntimeKind) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.kind() == kind)
            .cloned()
    }
}

#[derive(Clone)]
pub struct PluginSdk {
    adapters: RuntimeAdapterRegistry,
}

impl Default for PluginSdk {
    fn default() -> Self {
        Self {
            adapters: RuntimeAdapterRegistry::with_defaults(),
        }
    }
}

impl PluginSdk {
    pub fn new(adapters: RuntimeAdapterRegistry) -> Self {
        Self { adapters }
    }

    pub async fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> Result<PluginLoadPlan, PluginSdkError> {
        let adapter =
            self.adapters
                .find(descriptor.runtime.kind)
                .ok_or(PluginSdkError::UnsupportedRuntime {
                    kind: descriptor.runtime.kind,
                    reason: "no runtime adapter registered".to_string(),
                })?;
        adapter.plan_load(descriptor, host).await
    }
}

pub struct NativeRuntimeAdapter;
pub struct PythonRuntimeAdapter;
pub struct LuaRuntimeAdapter;

impl RuntimeAdapter for NativeRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Native
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        _host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let has_entry = !descriptor.runtime.entrypoint.trim().is_empty()
            || !descriptor.runtime.module.trim().is_empty();
        Box::pin(async move {
            if has_entry {
                Ok(PluginLoadPlan::ready(PluginRuntimeKind::Native))
            } else {
                Ok(PluginLoadPlan::deferred(
                    PluginRuntimeKind::Native,
                    "native plugin entrypoint is not declared",
                ))
            }
        })
    }
}

impl RuntimeAdapter for PythonRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Python
    }

    fn plan_load(
        &self,
        _descriptor: &PluginDescriptor,
        _host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        Box::pin(async move {
            Ok(PluginLoadPlan::deferred(
                PluginRuntimeKind::Python,
                "python runtime bridge is reserved for future pyo3 integration",
            ))
        })
    }
}

impl RuntimeAdapter for LuaRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Lua
    }

    fn plan_load(
        &self,
        _descriptor: &PluginDescriptor,
        _host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        Box::pin(async move {
            Ok(PluginLoadPlan::deferred(
                PluginRuntimeKind::Lua,
                "lua runtime bridge is reserved for future lua integration",
            ))
        })
    }
}

