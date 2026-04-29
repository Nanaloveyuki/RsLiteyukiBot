use super::*;
use crate::utils::runtime_settings::runtime_setting_value_pairs;

pub(super) fn config_adapters(doc: &AppConfigDoc) -> Option<&Vec<AdapterConfig>> {
    doc.rust
        .as_ref()
        .and_then(|section| section.adapters.as_ref())
        .or(doc.adapters.as_ref())
}

pub(super) fn config_connect(doc: &AppConfigDoc) -> Option<&ConnectConfigSection> {
    doc.connect.as_ref()
}

pub(super) fn config_tui_resume(doc: &AppConfigDoc) -> Option<&TuiResumeSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.tui.as_ref())
        .and_then(|tui| tui.resume.as_ref())
        .or(doc.tui.as_ref().and_then(|tui| tui.resume.as_ref()))
}

pub(super) fn config_i18n(doc: &AppConfigDoc) -> Option<&I18nConfigSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.i18n.as_ref())
        .or(doc.i18n.as_ref())
}

pub(super) fn config_llm(doc: &AppConfigDoc) -> Option<&LlmConfigSection> {
    doc.llm.as_ref()
}

pub(super) fn config_runtime(doc: &AppConfigDoc) -> Option<&RuntimeConfigSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.runtime.as_ref())
        .or(doc.runtime.as_ref())
}

pub(super) fn config_log(doc: &AppConfigDoc) -> Option<&LogConfigSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.log.as_ref())
        .or(doc.log.as_ref())
}

pub(crate) fn runtime_settings_values(doc: &AppConfigDoc) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let runtime = config_runtime(doc);
    let log = config_log(doc);

    for (key, value) in runtime_setting_value_pairs(
        runtime.and_then(|section| section.worker_count),
        runtime.and_then(|section| section.ingress_queue),
        runtime.and_then(|section| section.worker_queue),
        log.and_then(|section| section.mode.as_deref()),
        log.and_then(|section| section.level.as_deref()),
        log.and_then(|section| section.timezone.as_deref()),
        log.and_then(|section| section.timestamp_format.as_deref()),
        log.and_then(|section| section.timestamp_pattern.as_deref()),
    ) {
        values.insert(key.to_string(), value);
    }

    values
}

pub(super) fn config_onebot_v11(doc: &AppConfigDoc) -> Option<&OnebotV11ConfigSection> {
    doc.onebot_v11.as_ref()
}

pub(super) fn config_commands(doc: &AppConfigDoc) -> Option<&CommandConfigSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.commands.as_ref())
        .or(doc.commands.as_ref())
}

pub(super) fn config_plugins(doc: &AppConfigDoc) -> Option<&PluginConfigSection> {
    doc.rust
        .as_ref()
        .and_then(|section| section.plugins.as_ref())
        .or(doc.plugins.as_ref())
}

pub(crate) fn resolve_help_whitelist(doc: &AppConfigDoc) -> HashSet<String> {
    config_onebot_v11(doc)
        .map(|section| {
            section
                .whitelist
                .iter()
                .map(OnebotWhitelistEntry::as_token)
                .filter(|value| !value.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn normalize_disabled_command_entry(raw: &str) -> Option<String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return None;
    }

    let parts = normalized.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 2 {
        return None;
    }

    let scope = normalize_command_scope(parts[0])?;
    let command = normalize_command_name(parts[1])?;
    Some(format!("{scope} {command}"))
}

fn normalize_command_scope(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized == "tui" {
        return Some("tui".to_string());
    }

    let adapter = normalized
        .strip_prefix("adapter:")
        .unwrap_or(normalized.as_str())
        .trim();
    let canonical = match adapter
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .as_str()
    {
        "onebot11" | "onebotv11" => "onebot11",
        _ => return None,
    };

    Some(format!("adapter:{canonical}"))
}

fn normalize_command_name(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.starts_with('/') {
        Some(normalized)
    } else {
        Some(format!("/{normalized}"))
    }
}

pub(crate) fn resolve_disabled_scope_commands(doc: &AppConfigDoc) -> Vec<String> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    if let Some(commands) = config_commands(doc) {
        for entry in &commands.disabled {
            let Some(normalized) = normalize_disabled_command_entry(entry) else {
                continue;
            };
            if seen.insert(normalized.clone()) {
                entries.push(normalized);
            }
        }
    }
    entries
}

pub(super) fn normalize_plugin_id_entry(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(crate) fn resolve_disabled_plugins(doc: &AppConfigDoc) -> Vec<String> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    if let Some(plugins) = config_plugins(doc) {
        for entry in &plugins.disabled {
            let Some(normalized) = normalize_plugin_id_entry(entry) else {
                continue;
            };
            if seen.insert(normalized.clone()) {
                entries.push(normalized);
            }
        }
    }
    entries
}
