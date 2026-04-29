use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid};

use crate::{LogLevel, emit_console_log};

use super::{AppHostState, with_state_write};

const RESOURCE_USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppHostCpuUsage {
    pub system_percent: f32,
    pub process_percent: f32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppHostMemoryUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub process_bytes: u64,
    pub system_percent: f32,
    pub process_percent: f32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppHostResourceUsage {
    pub cpu: AppHostCpuUsage,
    pub memory: AppHostMemoryUsage,
}

pub(crate) fn spawn_resource_usage_sampler(state: Arc<RwLock<AppHostState>>) {
    let mut sampler = match ResourceUsageSampler::try_new() {
        Ok(sampler) => sampler,
        Err(err) => {
            let warning = format!("resource usage sampler unavailable: {err}");
            emit_console_log(LogLevel::Warn, "app.host", warning.as_str());
            with_state_write(&state, |host| host.push_warning(warning));
            return;
        }
    };

    with_state_write(&state, |host| host.set_resource_usage(sampler.sample()));

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(RESOURCE_USAGE_SAMPLE_INTERVAL);
        loop {
            ticker.tick().await;
            with_state_write(&state, |host| host.set_resource_usage(sampler.sample()));
        }
    });
}

struct ResourceUsageSampler {
    pid: sysinfo::Pid,
    system: System,
}

impl ResourceUsageSampler {
    fn try_new() -> Result<Self, String> {
        let pid = get_current_pid().map_err(|err| err.to_string())?;
        let mut system = System::new();
        system.refresh_memory();
        system.refresh_cpu_usage();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::new().with_cpu().with_memory(),
        );
        Ok(Self { pid, system })
    }

    fn sample(&mut self) -> AppHostResourceUsage {
        self.system.refresh_memory();
        self.system.refresh_cpu_usage();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::new().with_cpu().with_memory(),
        );

        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();
        let (process_cpu, process_memory) = self
            .system
            .process(self.pid)
            .map(|process| (process.cpu_usage(), process.memory()))
            .unwrap_or((0.0, 0));

        AppHostResourceUsage {
            cpu: AppHostCpuUsage {
                system_percent: normalize_percent(self.system.global_cpu_usage()),
                process_percent: normalize_percent(process_cpu),
            },
            memory: AppHostMemoryUsage {
                total_bytes: total_memory,
                used_bytes: used_memory,
                process_bytes: process_memory,
                system_percent: usage_percent(used_memory, total_memory),
                process_percent: usage_percent(process_memory, total_memory),
            },
        }
    }
}

pub(crate) fn usage_percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    normalize_percent(((used as f64 / total as f64) * 100.0) as f32)
}

pub(crate) fn normalize_percent(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 100.0)
}
