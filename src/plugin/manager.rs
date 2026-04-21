use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::comm::{ChannelRegistry, SharedStore};
use crate::core::{LifecycleContext, RuntimeTarget};
use crate::observability::Logger;
use crate::session::SessionRouter;

use super::loader::{PluginManifestError, PluginManifestLoader};
use super::sdk::{PluginHostBridge, PluginLoadPlan, PluginLoadState, PluginSdk};
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
    fn on_start(&self, _context: PluginContext) -> PluginFuture {
        Box::pin(async { Ok(()) })
    }
    fn on_health_check(&self, _context: PluginContext) -> PluginFuture {
        Box::pin(async { Ok(()) })
    }
    fn on_shutdown(&self, _context: PluginContext) -> PluginFuture {
        Box::pin(async { Ok(()) })
    }
    fn on_unload(&self, _context: PluginContext) -> PluginFuture {
        Box::pin(async { Ok(()) })
    }
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
pub struct PluginCatalogEntry {
    pub descriptor: PluginDescriptor,
    pub loaded: bool,
    pub load_state: Option<PluginLoadState>,
    pub load_reason: Option<String>,
}

impl LoadedPlugin {
    fn has_deferred_manifest_runtime(&self) -> bool {
        self.descriptor.manifest_path.is_some() && self.load_plan.state == PluginLoadState::Deferred
    }

    fn deferred_runtime_reason(&self) -> &str {
        self.load_plan
            .reason
            .as_deref()
            .unwrap_or("runtime bridge is deferred")
    }
}

#[derive(Debug, Clone)]
pub enum PluginLoadError {
    AlreadyRegistered(String),
    NotFound(String),
    AlreadyLoaded(String),
    Loading(String),
    Hook {
        id: String,
        reason: String,
    },
    Lifecycle {
        id: String,
        phase: &'static str,
        reason: String,
    },
    Sdk {
        id: String,
        reason: String,
    },
    Io(String),
    Parse(String),
}

impl std::fmt::Display for PluginLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered(id) => write!(f, "plugin '{}' already registered", id),
            Self::NotFound(id) => write!(f, "plugin '{}' not found", id),
            Self::AlreadyLoaded(id) => write!(f, "plugin '{}' already loaded", id),
            Self::Loading(id) => write!(f, "plugin '{}' is currently loading", id),
            Self::Hook { id, reason } => write!(f, "plugin '{}' load hook failed: {}", id, reason),
            Self::Lifecycle { id, phase, reason } => {
                write!(f, "plugin '{}' {} hook failed: {}", id, phase, reason)
            }
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
    loading: Arc<Mutex<HashSet<String>>>,
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
        let _loading_guard = self.begin_loading(id)?;

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

    pub fn plugin_catalog(&self) -> Vec<PluginCatalogEntry> {
        let loaded = self
            .loaded
            .read()
            .expect("plugin loaded lock should not be poisoned")
            .clone();
        let mut catalog = self
            .registry
            .read()
            .expect("plugin registry lock should not be poisoned")
            .values()
            .map(|plugin| {
                let descriptor = plugin.descriptor();
                let loaded_item = loaded.get(descriptor.metadata.id.as_str());
                PluginCatalogEntry {
                    descriptor,
                    loaded: loaded_item.is_some(),
                    load_state: loaded_item.map(|item| item.load_plan.state),
                    load_reason: loaded_item.and_then(|item| item.load_plan.reason.clone()),
                }
            })
            .collect::<Vec<_>>();
        catalog.sort_by(|left, right| {
            left.descriptor
                .metadata
                .id
                .cmp(&right.descriptor.metadata.id)
        });
        catalog
    }

    pub async fn start_loaded_plugins(
        &self,
        context: PluginContext,
    ) -> Result<(), PluginLoadError> {
        let mut loaded = self.loaded_plugins();
        loaded.sort_by_key(|plugin| plugin.loaded_at_ms);
        for item in loaded {
            let id = item.descriptor.metadata.id.clone();
            if item.has_deferred_manifest_runtime() {
                if let Some(logger) = &self.logger {
                    logger.warn_in(
                        MODULE_PLUGIN,
                        format!(
                            "plugin '{}' start skipped because manifest runtime is deferred: {}",
                            id,
                            item.deferred_runtime_reason()
                        ),
                    );
                }
                continue;
            }
            let plugin = self
                .registry
                .read()
                .expect("plugin registry lock should not be poisoned")
                .get(id.as_str())
                .cloned()
                .ok_or_else(|| PluginLoadError::NotFound(id.clone()))?;
            plugin.on_start(context.clone()).await.map_err(|reason| {
                PluginLoadError::Lifecycle {
                    id: id.clone(),
                    phase: "start",
                    reason,
                }
            })?;
            if let Some(logger) = &self.logger {
                logger.info_in(MODULE_PLUGIN, format!("plugin '{}' started", id));
            }
        }
        Ok(())
    }

    pub async fn shutdown_loaded_plugins(
        &self,
        context: PluginContext,
    ) -> Result<(), PluginLoadError> {
        let mut loaded = self.loaded_plugins();
        loaded.sort_by(|left, right| right.loaded_at_ms.cmp(&left.loaded_at_ms));

        let mut first_error: Option<PluginLoadError> = None;
        for item in loaded {
            let id = item.descriptor.metadata.id.clone();
            if item.has_deferred_manifest_runtime() {
                self.loaded
                    .write()
                    .expect("plugin loaded lock should not be poisoned")
                    .remove(id.as_str());

                if let Some(logger) = &self.logger {
                    logger.info_in(
                        MODULE_PLUGIN,
                        format!(
                            "plugin '{}' unloaded without runtime hooks because manifest runtime is deferred",
                            id
                        ),
                    );
                }
                continue;
            }
            let plugin = self
                .registry
                .read()
                .expect("plugin registry lock should not be poisoned")
                .get(id.as_str())
                .cloned();
            if let Some(plugin) = &plugin {
                if let Err(reason) = plugin.on_shutdown(context.clone()).await
                    && first_error.is_none()
                {
                    first_error = Some(PluginLoadError::Lifecycle {
                        id: id.clone(),
                        phase: "shutdown",
                        reason,
                    });
                }
            } else if first_error.is_none() {
                first_error = Some(PluginLoadError::NotFound(id.clone()));
            }

            if let Some(plugin) = &plugin
                && let Err(reason) = plugin.on_unload(context.clone()).await
                && first_error.is_none()
            {
                first_error = Some(PluginLoadError::Lifecycle {
                    id: id.clone(),
                    phase: "unload",
                    reason,
                });
            }

            self.loaded
                .write()
                .expect("plugin loaded lock should not be poisoned")
                .remove(id.as_str());

            if let Some(logger) = &self.logger {
                logger.info_in(MODULE_PLUGIN, format!("plugin '{}' shutdown complete", id));
            }
        }

        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    pub async fn health_check_loaded_plugins(
        &self,
        context: PluginContext,
    ) -> Result<(), PluginLoadError> {
        let mut loaded = self.loaded_plugins();
        loaded.sort_by_key(|plugin| plugin.loaded_at_ms);
        for item in loaded {
            let id = item.descriptor.metadata.id.clone();
            if item.has_deferred_manifest_runtime() {
                return Err(PluginLoadError::Lifecycle {
                    id,
                    phase: "health_check",
                    reason: format!(
                        "manifest runtime is deferred: {}",
                        item.deferred_runtime_reason()
                    ),
                });
            }
            let plugin = self
                .registry
                .read()
                .expect("plugin registry lock should not be poisoned")
                .get(id.as_str())
                .cloned()
                .ok_or_else(|| PluginLoadError::NotFound(id.clone()))?;
            plugin
                .on_health_check(context.clone())
                .await
                .map_err(|reason| PluginLoadError::Lifecycle {
                    id: id.clone(),
                    phase: "health_check",
                    reason,
                })?;
            if let Some(logger) = &self.logger {
                logger.info_in(
                    MODULE_PLUGIN,
                    format!("plugin '{}' health check passed", id),
                );
            }
        }
        Ok(())
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

    fn begin_loading(&self, id: &str) -> Result<PluginLoadGuard, PluginLoadError> {
        if self.is_loaded(id) {
            return Err(PluginLoadError::AlreadyLoaded(id.to_string()));
        }

        let mut loading = self
            .loading
            .lock()
            .expect("plugin loading lock should not be poisoned");
        if loading.contains(id) {
            return Err(PluginLoadError::Loading(id.to_string()));
        }
        if self.is_loaded(id) {
            return Err(PluginLoadError::AlreadyLoaded(id.to_string()));
        }
        loading.insert(id.to_string());
        Ok(PluginLoadGuard {
            id: id.to_string(),
            loading: Arc::clone(&self.loading),
        })
    }
}

struct PluginLoadGuard {
    id: String,
    loading: Arc<Mutex<HashSet<String>>>,
}

impl Drop for PluginLoadGuard {
    fn drop(&mut self) {
        self.loading
            .lock()
            .expect("plugin loading lock should not be poisoned")
            .remove(&self.id);
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
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            context.logger.info_in(
                MODULE_PLUGIN,
                format!(
                    "manifest plugin '{}' registered from {} (runtime={:?})",
                    plugin_id, path, runtime
                ),
            );
            match context.sdk.load_manifest_plugin(&descriptor, &context.host) {
                Ok(true) => {
                    context.logger.info_in(
                        MODULE_PLUGIN,
                        format!(
                            "manifest plugin '{}' runtime bridge activated (runtime={:?})",
                            plugin_id, runtime
                        ),
                    );
                }
                Ok(false) => {
                    context.logger.warn_in(
                        MODULE_PLUGIN,
                        format!(
                            "manifest plugin '{}' runtime bridge deferred (runtime={:?})",
                            plugin_id, runtime
                        ),
                    );
                }
                Err(err) => {
                    return Err(format!(
                        "manifest plugin '{}' runtime activation failed: {}",
                        plugin_id, err
                    ));
                }
            }
            Ok(())
        })
    }

    fn on_start(&self, context: PluginContext) -> PluginFuture {
        let plugin_id = self.descriptor.metadata.id.clone();
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            context
                .sdk
                .start_manifest_plugin(&descriptor)
                .map_err(|err| format!("manifest plugin '{}' start failed: {}", plugin_id, err))
        })
    }

    fn on_health_check(&self, context: PluginContext) -> PluginFuture {
        let plugin_id = self.descriptor.metadata.id.clone();
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            context
                .sdk
                .health_check_manifest_plugin(&descriptor)
                .map_err(|err| {
                    format!(
                        "manifest plugin '{}' health check failed: {}",
                        plugin_id, err
                    )
                })
        })
    }

    fn on_shutdown(&self, context: PluginContext) -> PluginFuture {
        let plugin_id = self.descriptor.metadata.id.clone();
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            context
                .sdk
                .shutdown_manifest_plugin(&descriptor)
                .map_err(|err| format!("manifest plugin '{}' shutdown failed: {}", plugin_id, err))
        })
    }

    fn on_unload(&self, context: PluginContext) -> PluginFuture {
        let plugin_id = self.descriptor.metadata.id.clone();
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            context
                .sdk
                .unload_manifest_plugin(&descriptor)
                .map_err(|err| format!("manifest plugin '{}' unload failed: {}", plugin_id, err))
        })
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
