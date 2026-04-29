use super::*;

pub(super) fn build_plugin_capability_support(
    service: &WebHostService,
    snapshot: &crate::PluginCapabilitySnapshot,
    plugin_id: &str,
    plugin_active: bool,
) -> PluginCapabilitySupportSummary {
    let cron_registered = !snapshot.cron_jobs.is_empty();
    let cron_enabled = snapshot.cron_jobs.iter().any(|job| job.enabled);
    let cron_executable = if plugin_active {
        service
            .runtime_host
            .as_ref()
            .and_then(|runtime_host| {
                run_async_for_web_host(runtime_host.plugin_has_executable_cron_jobs(plugin_id)).ok()
            })
            .unwrap_or_else(|| {
                snapshot
                    .cron_jobs
                    .iter()
                    .any(crate::llm::cron_task::cron_job_is_host_executable)
            })
    } else {
        false
    };
    PluginCapabilitySupportSummary {
        tools: build_tool_capability_support(snapshot, plugin_active),
        web_apis: build_web_api_capability_support(snapshot, plugin_active),
        cron_jobs: build_cron_capability_support(
            cron_registered,
            cron_enabled,
            cron_executable,
            plugin_active,
        ),
        tasks: build_registration_only_capability_support(
            !snapshot.tasks.is_empty(),
            plugin_active,
        ),
    }
}

fn build_tool_capability_support(
    snapshot: &crate::PluginCapabilitySnapshot,
    plugin_active: bool,
) -> PluginCapabilitySupportState {
    let registered = !snapshot.tools.is_empty();
    let executable = plugin_active && snapshot.tools.iter().any(|tool| tool.active);
    PluginCapabilitySupportState {
        registered,
        executable,
        persistent: false,
        active: executable,
        status: if !registered {
            "unsupported"
        } else if executable {
            "active"
        } else {
            "disabled"
        }
        .to_string(),
    }
}

fn build_web_api_capability_support(
    snapshot: &crate::PluginCapabilitySnapshot,
    plugin_active: bool,
) -> PluginCapabilitySupportState {
    let registered = !snapshot.web_apis.is_empty();
    let executable = registered && plugin_active;
    PluginCapabilitySupportState {
        registered,
        executable,
        persistent: false,
        active: executable,
        status: if !registered {
            "unsupported"
        } else if executable {
            "active"
        } else {
            "disabled"
        }
        .to_string(),
    }
}

fn build_registration_only_capability_support(
    registered: bool,
    plugin_active: bool,
) -> PluginCapabilitySupportState {
    PluginCapabilitySupportState {
        registered,
        executable: false,
        persistent: false,
        active: registered && plugin_active,
        status: if !registered {
            "unsupported"
        } else if plugin_active {
            "registered_only"
        } else {
            "disabled"
        }
        .to_string(),
    }
}

fn build_cron_capability_support(
    registered: bool,
    enabled: bool,
    executable: bool,
    plugin_active: bool,
) -> PluginCapabilitySupportState {
    PluginCapabilitySupportState {
        registered,
        executable,
        persistent: executable,
        active: registered && plugin_active && enabled,
        status: if !registered {
            "unsupported"
        } else if !plugin_active || !enabled {
            "disabled"
        } else if executable {
            "active"
        } else {
            "registered_only"
        }
        .to_string(),
    }
}

#[cfg(test)]
#[path = "support/tests.rs"]
mod tests;
