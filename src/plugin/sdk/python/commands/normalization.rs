use super::super::state::ScopedCommandKey;

pub(crate) fn normalize_tui_command_name(raw: &str) -> Option<String> {
    let first = raw.split_whitespace().next()?.trim();
    if first.is_empty() {
        return None;
    }
    let normalized = if first.starts_with('/') {
        first.to_ascii_lowercase()
    } else {
        format!("/{}", first.to_ascii_lowercase())
    };
    if normalized == "/" {
        None
    } else {
        Some(normalized)
    }
}

pub(super) fn normalize_plugin_scope(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let compact = trimmed.to_ascii_lowercase().replace([' ', '_', '-'], "");
    match compact.as_str() {
        "all" => Some("all".to_string()),
        "tui" => Some("tui".to_string()),
        "adapter:onebot11" | "adapter:onebotv11" | "adapteronebot11" | "onebot11" | "onebotv11" => {
            Some("adapter:onebot11".to_string())
        }
        _ => Some(trimmed.to_ascii_lowercase()),
    }
}

pub(super) fn normalize_plugin_scopes(raw_scopes: &[String]) -> Vec<String> {
    let mut scopes = Vec::new();
    for scope in raw_scopes {
        if let Some(normalized) = normalize_plugin_scope(scope)
            && !scopes.iter().any(|existing| existing == &normalized)
        {
            scopes.push(normalized);
        }
    }
    if scopes.is_empty() {
        scopes.push("all".to_string());
    }
    scopes
}

pub(super) fn plugin_scope_matches(scopes: &[String], scope: &str) -> bool {
    let Some(scope) = normalize_plugin_scope(scope) else {
        return false;
    };
    scopes
        .iter()
        .filter_map(|entry| normalize_plugin_scope(entry))
        .any(|entry| entry == "all" || entry == scope)
}

pub(super) fn normalize_scoped_command_key(scope: &str, command: &str) -> Option<ScopedCommandKey> {
    Some(ScopedCommandKey {
        scope: normalize_plugin_scope(scope)?,
        command: normalize_tui_command_name(command)?,
    })
}
