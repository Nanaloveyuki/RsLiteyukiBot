use std::collections::HashMap;

use super::*;

pub(super) fn plugin_capabilities_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginCapabilitiesPayload, String> {
    if plugin_id.trim().is_empty() {
        return Err("missing plugin id".to_string());
    }
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "plugin runtime host is unavailable".to_string())?;
    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.descriptor.metadata.id == plugin_id)
        .ok_or_else(|| "plugin not found".to_string())?;
    let runtime_kind = entry.descriptor.runtime.kind;
    let active = entry.loaded && !plugin_is_disabled(&catalog.disabled_plugin_ids, plugin_id);
    let snapshot = run_async_for_web_host(runtime_host.plugin_capability_snapshot(plugin_id))
        .map_err(|err| format!("failed to read plugin capability snapshot: {err}"))?
        .unwrap_or_else(|| empty_plugin_capability_snapshot(plugin_id, runtime_kind));
    let support =
        super::support::build_plugin_capability_support(service, &snapshot, plugin_id, active);
    Ok(PluginCapabilitiesPayload {
        plugin_id: plugin_id.to_string(),
        runtime_kind,
        support,
        snapshot,
    })
}

pub(super) fn all_plugin_capabilities_payload(
    service: &WebHostService,
) -> Result<Vec<PluginCapabilitiesPayload>, String> {
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "plugin runtime host is unavailable".to_string())?;
    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let snapshots = run_async_for_web_host(runtime_host.all_plugin_capability_snapshots())
        .map_err(|err| format!("failed to read plugin capability snapshots: {err}"))?;
    let snapshot_map: HashMap<String, crate::PluginCapabilitySnapshot> = HashMap::from_iter(
        snapshots
            .into_iter()
            .map(|snapshot| (snapshot.plugin_id.clone(), snapshot)),
    );

    let disabled_plugin_ids = catalog.disabled_plugin_ids;
    let mut payloads = catalog
        .entries
        .into_iter()
        .map(|entry| {
            let plugin_id = entry.descriptor.metadata.id.clone();
            let runtime_kind = entry.descriptor.runtime.kind;
            let active =
                entry.loaded && !plugin_is_disabled(&disabled_plugin_ids, plugin_id.as_str());
            let snapshot = snapshot_map
                .get(plugin_id.as_str())
                .cloned()
                .unwrap_or_else(|| {
                    empty_plugin_capability_snapshot(plugin_id.as_str(), runtime_kind)
                });
            PluginCapabilitiesPayload {
                plugin_id: plugin_id.clone(),
                runtime_kind,
                support: super::support::build_plugin_capability_support(
                    service,
                    &snapshot,
                    plugin_id.as_str(),
                    active,
                ),
                snapshot,
            }
        })
        .collect::<Vec<_>>();
    payloads.sort_by(|left, right| left.plugin_id.cmp(&right.plugin_id));
    Ok(payloads)
}

pub(super) fn plugin_runtime_state_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginRuntimeStatePayload, String> {
    let payload = plugin_capabilities_payload(service, plugin_id)?;
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "plugin runtime host is unavailable".to_string())?;
    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.descriptor.metadata.id == plugin_id)
        .ok_or_else(|| "plugin not found".to_string())?;
    let enabled = !plugin_is_disabled(&catalog.disabled_plugin_ids, plugin_id);
    let active = entry.loaded && enabled;
    let executable_bindings = build_runtime_binding_summary(&payload.support);

    Ok(PluginRuntimeStatePayload {
        plugin_id: plugin_id.to_string(),
        runtime_kind: payload.runtime_kind,
        loaded: entry.loaded,
        enabled,
        active,
        snapshot_extracted: snapshot_has_capabilities(&payload.snapshot),
        executable_bindings,
        scheduler_status: if !enabled && payload.support.cron_jobs.registered {
            "disabled".to_string()
        } else {
            run_async_for_web_host(runtime_host.plugin_cron_scheduler_status(plugin_id))
                .unwrap_or_else(|_| "unsupported".to_string())
        },
        task_runtime_status: if payload.support.tasks.registered {
            "registered_only".to_string()
        } else {
            "unsupported".to_string()
        },
    })
}

pub(super) fn plugin_diagnostics_payload(
    service: &WebHostService,
    plugin_id: &str,
) -> Result<PluginDiagnosticsPayload, String> {
    let payload = plugin_capabilities_payload(service, plugin_id)?;
    let runtime_host = service
        .runtime_host
        .as_ref()
        .ok_or_else(|| "plugin runtime host is unavailable".to_string())?;
    let catalog = run_async_for_web_host(runtime_host.plugin_catalog_snapshot());
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.descriptor.metadata.id == plugin_id)
        .ok_or_else(|| "plugin not found".to_string())?;
    let diagnostics = run_async_for_web_host(runtime_host.plugin_runtime_diagnostics(plugin_id))?
        .unwrap_or_else(|| crate::PluginRuntimeDiagnostics {
            plugin_id: plugin_id.to_string(),
            ..crate::PluginRuntimeDiagnostics::default()
        });

    Ok(PluginDiagnosticsPayload {
        plugin_id: plugin_id.to_string(),
        runtime_kind: payload.runtime_kind,
        load_state: if entry.loaded {
            "loaded".to_string()
        } else {
            "unloaded".to_string()
        },
        snapshot_extracted: snapshot_has_capabilities(&payload.snapshot),
        executable_bindings: build_runtime_binding_summary(&payload.support),
        scheduler_status: if !entry.loaded && payload.support.cron_jobs.registered {
            "disabled".to_string()
        } else {
            run_async_for_web_host(runtime_host.plugin_cron_scheduler_status(plugin_id))
                .unwrap_or_else(|_| "unsupported".to_string())
        },
        last_web_api_dispatch: diagnostics.last_web_api_dispatch,
        last_tool_execution: diagnostics.last_tool_execution,
        last_cron_execution: diagnostics.last_cron_execution,
    })
}

fn empty_plugin_capability_snapshot(
    plugin_id: &str,
    runtime_kind: crate::PluginRuntimeKind,
) -> crate::PluginCapabilitySnapshot {
    crate::PluginCapabilitySnapshot {
        plugin_id: plugin_id.to_string(),
        runtime_kind,
        tools: Vec::new(),
        web_apis: Vec::new(),
        cron_jobs: Vec::new(),
        tasks: Vec::new(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn snapshot_has_capabilities(snapshot: &crate::PluginCapabilitySnapshot) -> bool {
    !snapshot.tools.is_empty()
        || !snapshot.web_apis.is_empty()
        || !snapshot.cron_jobs.is_empty()
        || !snapshot.tasks.is_empty()
}

fn plugin_is_disabled(disabled_plugin_ids: &[String], plugin_id: &str) -> bool {
    disabled_plugin_ids.iter().any(|id| id == plugin_id)
}

fn build_runtime_binding_summary(
    support: &PluginCapabilitySupportSummary,
) -> PluginRuntimeBindingSummary {
    PluginRuntimeBindingSummary {
        tools: support.tools.executable,
        web_apis: support.web_apis.executable,
        cron_jobs: support.cron_jobs.executable,
        tasks: support.tasks.executable,
    }
}
