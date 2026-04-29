use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub(super) fn matches(&self, context: &LifecycleContext) -> bool {
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
