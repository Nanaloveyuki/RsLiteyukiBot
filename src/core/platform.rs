use super::{BotRuntimeConfig, RuntimeCapabilities, RuntimeFlavor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTarget {
    Cli,
    Web,
    Tauri2,
    Docker,
    CliWeb,
    DockerWeb,
}

impl RuntimeTarget {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cli" => Some(Self::Cli),
            "web" => Some(Self::Web),
            "tauri" | "tauri2" | "desktop" => Some(Self::Tauri2),
            "docker" => Some(Self::Docker),
            "cli-web" | "web-cli" => Some(Self::CliWeb),
            "docker-web" | "web-docker" => Some(Self::DockerWeb),
            _ => None,
        }
    }

    pub fn runtime_flavor(self) -> RuntimeFlavor {
        match self {
            Self::Cli => RuntimeFlavor::Cli,
            Self::Web => RuntimeFlavor::Web,
            Self::Tauri2 => RuntimeFlavor::DesktopTauri2,
            Self::Docker => RuntimeFlavor::Docker,
            Self::CliWeb => RuntimeFlavor::Custom("cli-web".to_string()),
            Self::DockerWeb => RuntimeFlavor::Custom("docker-web".to_string()),
        }
    }

    pub fn capabilities(self) -> RuntimeCapabilities {
        match self {
            Self::Cli => RuntimeCapabilities {
                cli: true,
                ..RuntimeCapabilities::default()
            },
            Self::Web => RuntimeCapabilities {
                cli: false,
                web: true,
                ..RuntimeCapabilities::default()
            },
            Self::Tauri2 => RuntimeCapabilities {
                cli: false,
                desktop_tauri2: true,
                ..RuntimeCapabilities::default()
            },
            Self::Docker => RuntimeCapabilities {
                cli: false,
                docker: true,
                ..RuntimeCapabilities::default()
            },
            Self::CliWeb => RuntimeCapabilities {
                cli: true,
                web: true,
                ..RuntimeCapabilities::default()
            },
            Self::DockerWeb => RuntimeCapabilities {
                cli: false,
                web: true,
                docker: true,
                ..RuntimeCapabilities::default()
            },
        }
    }

    pub fn tune_runtime_config(self, mut config: BotRuntimeConfig) -> BotRuntimeConfig {
        let available = available_parallelism();
        let tuned_workers = match self {
            Self::Cli => available.max(2),
            Self::Web => available.max(2),
            // Desktop runtime keeps fewer workers to reduce UI-thread pressure and memory spikes.
            Self::Tauri2 => available.clamp(2, 4),
            Self::Docker => available.max(2),
            Self::CliWeb => available.max(2),
            Self::DockerWeb => available.max(2),
        };

        if config.worker_count == BotRuntimeConfig::default().worker_count {
            config.worker_count = tuned_workers;
        }

        if matches!(self, Self::Web | Self::CliWeb | Self::DockerWeb) && config.ingress_queue < 2048
        {
            config.ingress_queue = 2048;
        }

        if matches!(self, Self::DockerWeb) && config.worker_queue < 512 {
            config.worker_queue = 512;
        }

        config
    }
}

impl Default for RuntimeTarget {
    fn default() -> Self {
        Self::Cli
    }
}

fn available_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4)
}
