use std::collections::{HashMap, HashSet};

pub(crate) fn normalize_non_empty_string(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub(crate) fn normalize_lowercase_non_empty_string(raw: &str) -> Option<String> {
    normalize_non_empty_string(raw).map(|value| value.to_ascii_lowercase())
}

pub(crate) fn normalize_provider_url(raw: &str) -> Option<String> {
    let value = raw.trim().trim_end_matches('/');
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub(crate) fn normalize_string_entries_preserve_order(entries: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for raw in entries {
        let Some(value) = normalize_non_empty_string(raw.as_str()) else {
            continue;
        };
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

pub(crate) fn normalize_provider_url_entries(entries: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for raw in entries {
        let Some(value) = normalize_provider_url(raw.as_str()) else {
            continue;
        };
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

pub(crate) fn normalize_headers_map(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if key.is_empty() || value.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect()
}

pub(crate) fn parse_llm_key_list(raw: &str) -> Vec<String> {
    let values = raw
        .split([',', ';', '\n', '\r'])
        .map(str::to_string)
        .collect::<Vec<_>>();
    normalize_string_entries_preserve_order(values.as_slice())
}

pub(crate) fn collect_llm_api_keys(
    api_keys: Option<&Vec<String>>,
    api_key: Option<&str>,
) -> Vec<String> {
    let mut keys = api_keys
        .map(|values| normalize_string_entries_preserve_order(values.as_slice()))
        .unwrap_or_default();
    if let Some(value) = api_key.and_then(normalize_non_empty_string) {
        if !keys.iter().any(|existing| existing == &value) {
            keys.push(value);
        }
    }
    keys
}
