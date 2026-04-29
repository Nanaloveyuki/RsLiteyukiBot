use super::*;
use sysinfo::System;
use tokio::time::MissedTickBehavior;

fn round_metric(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

fn bytes_to_mebibytes(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

pub(super) fn arch_label() -> String {
    format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH)
}

fn current_cpu_profile() -> (String, usize, f32) {
    let mut system = System::new();
    system.refresh_cpu_all();

    let cpus = system.cpus();
    let model = cpus
        .iter()
        .find_map(|cpu| {
            let brand = cpu.brand().trim();
            (!brand.is_empty()).then(|| brand.to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string());
    let detected_cores = cpus.len();
    let fallback_cores = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1);
    let core_count = detected_cores.max(fallback_cores);
    let speed_ghz = cpus
        .iter()
        .find_map(|cpu| {
            let frequency_mhz = cpu.frequency();
            (frequency_mhz > 0).then_some(frequency_mhz as f32 / 1000.0)
        })
        .map(round_metric)
        .unwrap_or(0.0);

    (model, core_count, speed_ghz)
}

pub(super) fn napcat_system_status(snapshot: &AppHostSnapshot) -> serde_json::Value {
    let (cpu_model, cpu_core_count, cpu_speed_ghz) = current_cpu_profile();

    serde_json::json!({
        "cpu": {
            "core": cpu_core_count,
            "model": cpu_model,
            "speed": cpu_speed_ghz,
            "usage": {
                "system": round_metric(snapshot.resource_usage.cpu.system_percent),
                "qq": round_metric(snapshot.resource_usage.cpu.process_percent)
            }
        },
        "memory": {
            "total": bytes_to_mebibytes(snapshot.resource_usage.memory.total_bytes),
            "usage": {
                "system": bytes_to_mebibytes(snapshot.resource_usage.memory.used_bytes),
                "qq": bytes_to_mebibytes(snapshot.resource_usage.memory.process_bytes)
            }
        },
        "arch": arch_label()
    })
}

pub(super) async fn stream_realtime_logs(
    _service: &WebHostService,
    mut socket: TcpStream,
) -> io::Result<()> {
    write_sse_headers(&mut socket).await?;
    let mut ticker = tokio::time::interval(REALTIME_STREAM_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut previous_entries = Vec::new();
    let mut keepalive_ticks = 0usize;

    loop {
        ticker.tick().await;
        let current_entries = recent_buffered_logs(REALTIME_LOG_STREAM_LIMIT);
        let appended = if previous_entries.is_empty() {
            current_entries.clone()
        } else {
            appended_log_entries(previous_entries.as_slice(), current_entries.as_slice())
        };

        if appended.is_empty() {
            keepalive_ticks += 1;
            if keepalive_ticks >= SSE_KEEPALIVE_TICKS {
                write_sse_comment(&mut socket, "keep-alive").await?;
                keepalive_ticks = 0;
            }
        } else {
            keepalive_ticks = 0;
            let payload = serde_json::json!({
                "level": aggregate_log_level(appended.as_slice()),
                "message": appended
                    .iter()
                    .map(|entry| entry.line.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            write_sse_event(
                &mut socket,
                &serde_json::to_string(&payload).unwrap_or_else(|_| {
                    "{\"level\":\"info\",\"message\":\"log serialization error\"}".to_string()
                }),
            )
            .await?;
        }

        previous_entries = current_entries;
    }
}

pub(super) async fn stream_system_status(
    service: &WebHostService,
    mut socket: TcpStream,
) -> io::Result<()> {
    write_sse_headers(&mut socket).await?;
    let mut ticker = tokio::time::interval(REALTIME_STREAM_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        let snapshot = (service.snapshot_provider)();
        let payload = serde_json::to_string(&napcat_system_status(&snapshot))
            .unwrap_or_else(|_| "{}".to_string());
        write_sse_event(&mut socket, payload.as_str()).await?;
    }
}

pub(super) fn appended_log_entries(
    previous: &[BufferedLogEntry],
    current: &[BufferedLogEntry],
) -> Vec<BufferedLogEntry> {
    let max_overlap = previous.len().min(current.len());
    for overlap in (0..=max_overlap).rev() {
        if previous[previous.len().saturating_sub(overlap)..] == current[..overlap] {
            return current[overlap..].to_vec();
        }
    }
    current.to_vec()
}

pub(super) fn aggregate_log_level(entries: &[BufferedLogEntry]) -> &'static str {
    let mut highest = LogLevel::Info;
    for entry in entries {
        if let Some(level) = LogLevel::parse(entry.level.as_str())
            && level > highest
        {
            highest = level;
        }
    }
    match highest {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}
