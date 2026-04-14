use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::comm::{ChannelRegistry, SharedStore};
use crate::core::{LifecycleContext, RuntimeTarget};
use crate::observability::Logger;
use crate::session::SessionRouter;

use super::loader::{PluginManifestError, PluginManifestLoader};
use super::sdk::{PluginHostBridge, PluginLoadPlan, PluginSdk};
use super::{PluginDescriptor, PluginMetadata};

const MODULE_PLUGIN: &str = "plugin.manager";

pub type PluginFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;

pub trait Plugin: Send + Sync {
    fn id(&self) -> &str;
    fn metadata(&self) -> PluginMetadata;
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor::from_metadata(self.metadata())
    }
    fn on_load(&self, context: PluginContext) -> PluginFuture;
}

#[derive(Clone)]
pub struct PluginContext {
    pub target: RuntimeTarget,
    pub lifecycle: Arc<LifecycleContext>,
    pub channels: ChannelRegistry,
    pub shared_store: SharedStore,
    pub session_router: SessionRouter,
    pub logger: Logger,
    pub sdk: PluginSdk,
    pub host: PluginHostBridge,
}

#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub descriptor: PluginDescriptor,
    pub load_plan: PluginLoadPlan,
    pub loaded_at_ms: u128,
}

#[derive(Debug, Clone)]
pub enum PluginLoadError {
    AlreadyRegistered(String),
    NotFound(String),
    AlreadyLoaded(String),
    Hook { id: String, reason: String },
    Sdk { id: String, reason: String },
    Io(String),
    Parse(String),
}

impl std::fmt::Display for PluginLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered(id) => write!(f, "plugin '{}' already registered", id),
            Self::NotFound(id) => write!(f, "plugin '{}' not found", id),
            Self::AlreadyLoaded(id) => write!(f, "plugin '{}' already loaded", id),
            Self::Hook { id, reason } => write!(f, "plugin '{}' load hook failed: {}", id, reason),
            Self::Sdk { id, reason } => {
                write!(f, "plugin '{}' sdk planning failed: {}", id, reason)
            }
            Self::Io(message) => write!(f, "plugin IO error: {}", message),
            Self::Parse(message) => write!(f, "plugin parse error: {}", message),
        }
    }
}

impl std::error::Error for PluginLoadError {}

impl From<PluginManifestError> for PluginLoadError {
    fn from(value: PluginManifestError) -> Self {
        match value {
            PluginManifestError::Io(message) => Self::Io(message),
            PluginManifestError::Parse(message) => Self::Parse(message),
        }
    }
}

#[derive(Clone, Default)]
pub struct PluginManager {
    registry: Arc<RwLock<HashMap<String, Arc<dyn Plugin>>>>,
    loaded: Arc<RwLock<HashMap<String, LoadedPlugin>>>,
    logger: Option<Logger>,
}

impl PluginManager {
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

    pub fn register_plugin<P>(&self, plugin: P) -> Result<(), PluginLoadError>
    where
        P: Plugin + 'static,
    {
        self.register_plugin_arc(Arc::new(plugin))
    }

    pub fn register_plugin_arc(&self, plugin: Arc<dyn Plugin>) -> Result<(), PluginLoadError> {
        let id = plugin.id().to_string();
        let mut lock = self
            .registry
            .write()
            .expect("plugin registry lock should not be poisoned");
        if lock.contains_key(&id) {
            return Err(PluginLoadError::AlreadyRegistered(id));
        }
        if let Some(logger) = &self.logger {
            logger.debug_in(MODULE_PLUGIN, format!("register plugin '{}'", id));
        }
        lock.insert(id, plugin);
        Ok(())
    }

    pub fn has_plugin(&self, id: &str) -> bool {
        self.registry
            .read()
            .expect("plugin registry lock should not be poisoned")
            .contains_key(id)
    }

    pub async fn load_plugin(
        &self,
        id: &str,
        context: PluginContext,
    ) -> Result<LoadedPlugin, PluginLoadError> {
        if self.is_loaded(id) {
            return Err(PluginLoadError::AlreadyLoaded(id.to_string()));
        }

        let plugin = self
            .registry
            .read()
            .expect("plugin registry lock should not be poisoned")
            .get(id)
            .cloned()
            .ok_or_else(|| PluginLoadError::NotFound(id.to_string()))?;

        let descriptor = plugin.descriptor();
        let load_plan = context
            .sdk
            .plan_load(&descriptor, &context.host)
            .await
            .map_err(|err| PluginLoadError::Sdk {
                id: id.to_string(),
                reason: err.to_string(),
            })?;

        plugin
            .on_load(context.clone())
            .await
            .map_err(|reason| PluginLoadError::Hook {
                id: id.to_string(),
                reason,
            })?;

        let loaded = LoadedPlugin {
            descriptor,
            load_plan,
            loaded_at_ms: now_millis(),
        };
        self.loaded
            .write()
            .expect("plugin loaded lock should not be poisoned")
            .insert(id.to_string(), loaded.clone());

        if let Some(logger) = &self.logger {
            logger.info_in(
                MODULE_PLUGIN,
                format!(
                    "plugin '{}' loaded (state={:?})",
                    id, loaded.load_plan.state
                ),
            );
        }
        Ok(loaded)
    }

    pub async fn load_plugins<I>(
        &self,
        ids: I,
        context: PluginContext,
    ) -> Result<Vec<LoadedPlugin>, PluginLoadError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut loaded = Vec::new();
        for id in ids {
            if self.is_loaded(&id) {
                continue;
            }
            loaded.push(self.load_plugin(&id, context.clone()).await?);
        }
        Ok(loaded)
    }

    pub fn is_loaded(&self, id: &str) -> bool {
        self.loaded
            .read()
            .expect("plugin loaded lock should not be poisoned")
            .contains_key(id)
    }

    pub fn loaded_plugins(&self) -> Vec<LoadedPlugin> {
        self.loaded
            .read()
            .expect("plugin loaded lock should not be poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn discover_manifest_plugins_in_dirs<I, P>(
        &self,
        dirs: I,
    ) -> Result<Vec<String>, PluginLoadError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<std::path::Path>,
    {
        let manifests = PluginManifestLoader::discover_in_dirs(dirs)?;
        let mut ids = Vec::new();
        for manifest in manifests {
            let id = manifest.descriptor.metadata.id.clone();
            if self.has_plugin(&id) {
                continue;
            }
            self.register_plugin(ManifestPlugin {
                descriptor: manifest.descriptor,
                manifest_path: manifest.path,
            })?;
            ids.push(id);
        }
        Ok(ids)
    }
}

struct ManifestPlugin {
    descriptor: PluginDescriptor,
    manifest_path: PathBuf,
}

impl Plugin for ManifestPlugin {
    fn id(&self) -> &str {
        &self.descriptor.metadata.id
    }

    fn metadata(&self) -> PluginMetadata {
        self.descriptor.metadata.clone()
    }

    fn descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }

    fn on_load(&self, context: PluginContext) -> PluginFuture {
        let plugin_id = self.descriptor.metadata.id.clone();
        let runtime = self.descriptor.runtime.kind;
        let path = self.manifest_path.display().to_string();
        Box::pin(async move {
            context.logger.info_in(
                MODULE_PLUGIN,
                format!(
                    "manifest plugin '{}' registered from {} (runtime={:?})",
                    plugin_id, path, runtime
                ),
            );
            Ok(())
        })
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
