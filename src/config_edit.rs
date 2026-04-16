use std::path::Path;

pub fn persist_onebot_v11_whitelist(path: &Path, entries: &[String]) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|raw| raw.to_str())
        .map(|raw| raw.to_ascii_lowercase());

    let normalized = normalize_entries(entries);
    let updated = match ext.as_deref() {
        Some("yaml") | Some("yml") => update_yaml_document(&content, &normalized),
        Some("toml") => update_toml_document(&content, &normalized),
        _ => {
            return Err(format!(
                "unsupported config extension for {} (expected .yaml/.yml/.toml)",
                path.display()
            ));
        }
    };

    std::fs::write(path, updated)
        .map_err(|err| format!("failed to write config {}: {err}", path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct LlmConfigPatch {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_keys: Option<Vec<String>>,
}

pub fn persist_llm_config(path: &Path, patch: &LlmConfigPatch) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|raw| raw.to_str())
        .map(|raw| raw.to_ascii_lowercase());
    let normalized_patch = normalize_llm_patch(patch);

    let updated = match ext.as_deref() {
        Some("yaml") | Some("yml") => update_yaml_llm_document(&content, &normalized_patch),
        Some("toml") => update_toml_llm_document(&content, &normalized_patch),
        _ => {
            return Err(format!(
                "unsupported config extension for {} (expected .yaml/.yml/.toml)",
                path.display()
            ));
        }
    };

    std::fs::write(path, updated)
        .map_err(|err| format!("failed to write config {}: {err}", path.display()))?;
    Ok(())
}

fn normalize_entries(entries: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = entries
        .iter()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_llm_patch(patch: &LlmConfigPatch) -> LlmConfigPatch {
    let provider = patch
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let model = patch
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let api_keys = patch
        .api_keys
        .as_ref()
        .map(|keys| normalize_entries_preserve_order(keys))
        .filter(|keys| !keys.is_empty());

    LlmConfigPatch {
        enabled: patch.enabled,
        provider,
        model,
        api_keys,
    }
}

fn normalize_entries_preserve_order(entries: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();
    for raw in entries {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let value = raw.to_string();
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

fn update_yaml_document(content: &str, entries: &[String]) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();

    let section_index = lines.iter().position(|line| {
        matches!(
            line.trim(),
            "onebot-v11:" | "'onebot-v11':" | "\"onebot-v11\":"
        )
    });

    if let Some(section_start) = section_index {
        let section_indent = leading_spaces(lines[section_start].as_str());
        let section_end = find_yaml_section_end(&lines, section_start, section_indent);
        let whitelist_index = (section_start + 1..section_end).find(|&idx| {
            let trimmed = lines[idx].trim_start();
            trimmed.starts_with("whitelist:")
        });

        if let Some(index) = whitelist_index {
            let whitelist_indent = leading_spaces(lines[index].as_str());
            let mut block_end = index + 1;
            while block_end < section_end {
                let line = lines[block_end].as_str();
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                let indent = leading_spaces(line);
                if indent <= whitelist_indent {
                    break;
                }
                let trimmed_start = line.trim_start();
                if trimmed_start.starts_with('-') || trimmed_start.starts_with('#') {
                    block_end += 1;
                    continue;
                }
                break;
            }
            lines.splice(
                index..block_end,
                render_yaml_whitelist(whitelist_indent, entries),
            );
        } else {
            lines.splice(
                section_end..section_end,
                render_yaml_whitelist(section_indent + 2, entries),
            );
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("onebot-v11:".to_string());
        lines.extend(render_yaml_whitelist(2, entries));
    }

    join_lines(&lines, newline, trailing_newline)
}

fn find_yaml_section_end(lines: &[String], section_start: usize, section_indent: usize) -> usize {
    for (idx, line) in lines.iter().enumerate().skip(section_start + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = leading_spaces(line.as_str());
        if indent <= section_indent {
            return idx;
        }
    }
    lines.len()
}

fn render_yaml_whitelist(indent: usize, entries: &[String]) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if entries.is_empty() {
        return vec![format!("{prefix}whitelist: []")];
    }
    let mut lines = vec![format!("{prefix}whitelist:")];
    for entry in entries {
        let escaped = entry.replace('\'', "''");
        lines.push(format!("{prefix}  - '{escaped}'"));
    }
    lines
}

fn update_toml_document(content: &str, entries: &[String]) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    let table_index = lines
        .iter()
        .position(|line| matches!(line.trim(), "[onebot-v11]" | "[onebot_v11]"));
    let whitelist_line = render_toml_whitelist(entries);

    if let Some(table_start) = table_index {
        let table_end = find_toml_table_end(&lines, table_start);
        let existing = (table_start + 1..table_end).find(|&idx| {
            let trimmed = lines[idx].trim_start();
            trimmed.starts_with("whitelist") && trimmed.contains('=')
        });
        if let Some(index) = existing {
            lines[index] = whitelist_line;
        } else {
            lines.splice(table_end..table_end, vec![whitelist_line]);
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("[onebot-v11]".to_string());
        lines.push(whitelist_line);
    }

    join_lines(&lines, newline, trailing_newline)
}

fn update_yaml_llm_document(content: &str, patch: &LlmConfigPatch) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();

    let section_index = lines
        .iter()
        .position(|line| matches!(line.trim(), "llm:" | "'llm':" | "\"llm\":"));
    let section_start = if let Some(index) = section_index {
        index
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("llm:".to_string());
        lines.len() - 1
    };
    let section_indent = leading_spaces(lines[section_start].as_str());

    if let Some(enabled) = patch.enabled {
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "enabled",
            &enabled.to_string(),
        );
    }
    if let Some(provider) = patch.provider.as_deref() {
        let escaped = provider.replace('\'', "''");
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "provider",
            &format!("'{escaped}'"),
        );
    }
    if let Some(model) = patch.model.as_deref() {
        let escaped = model.replace('\'', "''");
        upsert_yaml_scalar(
            &mut lines,
            section_start,
            section_indent,
            "model",
            &format!("'{escaped}'"),
        );
    }
    if let Some(api_keys) = patch.api_keys.as_ref() {
        upsert_yaml_list(
            &mut lines,
            section_start,
            section_indent,
            "api_keys",
            api_keys,
        );
    }

    join_lines(&lines, newline, trailing_newline)
}

fn upsert_yaml_scalar(
    lines: &mut Vec<String>,
    section_start: usize,
    section_indent: usize,
    key: &str,
    value: &str,
) {
    let section_end = find_yaml_section_end(lines, section_start, section_indent);
    let key_indent = section_indent + 2;
    let key_index = (section_start + 1..section_end).find(|&idx| {
        let line = lines[idx].as_str();
        leading_spaces(line) == key_indent && line.trim_start().starts_with(&format!("{key}:"))
    });

    let rendered = format!("{}{}: {}", " ".repeat(key_indent), key, value);
    if let Some(index) = key_index {
        let block_end = find_yaml_key_block_end(lines, index, section_end);
        lines.splice(index..block_end, vec![rendered]);
    } else {
        lines.splice(section_end..section_end, vec![rendered]);
    }
}

fn upsert_yaml_list(
    lines: &mut Vec<String>,
    section_start: usize,
    section_indent: usize,
    key: &str,
    entries: &[String],
) {
    let section_end = find_yaml_section_end(lines, section_start, section_indent);
    let key_indent = section_indent + 2;
    let key_index = (section_start + 1..section_end).find(|&idx| {
        let line = lines[idx].as_str();
        leading_spaces(line) == key_indent && line.trim_start().starts_with(&format!("{key}:"))
    });

    let rendered = render_yaml_string_list(key_indent, key, entries);
    if let Some(index) = key_index {
        let block_end = find_yaml_key_block_end(lines, index, section_end);
        lines.splice(index..block_end, rendered);
    } else {
        lines.splice(section_end..section_end, rendered);
    }
}

fn find_yaml_key_block_end(lines: &[String], key_index: usize, section_end: usize) -> usize {
    let key_indent = leading_spaces(lines[key_index].as_str());
    let mut block_end = key_index + 1;
    while block_end < section_end {
        let line = lines[block_end].as_str();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            block_end += 1;
            continue;
        }
        let indent = leading_spaces(line);
        if indent <= key_indent {
            break;
        }
        block_end += 1;
    }
    block_end
}

fn render_yaml_string_list(indent: usize, key: &str, entries: &[String]) -> Vec<String> {
    let prefix = " ".repeat(indent);
    if entries.is_empty() {
        return vec![format!("{prefix}{key}: []")];
    }
    let mut lines = vec![format!("{prefix}{key}:")];
    for entry in entries {
        let escaped = entry.replace('\'', "''");
        lines.push(format!("{prefix}  - '{escaped}'"));
    }
    lines
}

fn update_toml_llm_document(content: &str, patch: &LlmConfigPatch) -> String {
    let newline = detect_newline(content);
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();

    let table_index = lines.iter().position(|line| line.trim() == "[llm]");
    let table_start = if let Some(index) = table_index {
        index
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("[llm]".to_string());
        lines.len() - 1
    };

    if let Some(enabled) = patch.enabled {
        upsert_toml_llm_key(&mut lines, table_start, "enabled", &enabled.to_string());
    }
    if let Some(provider) = patch.provider.as_deref() {
        let escaped = provider.replace('\\', "\\\\").replace('"', "\\\"");
        upsert_toml_llm_key(
            &mut lines,
            table_start,
            "provider",
            &format!("\"{escaped}\""),
        );
    }
    if let Some(model) = patch.model.as_deref() {
        let escaped = model.replace('\\', "\\\\").replace('"', "\\\"");
        upsert_toml_llm_key(&mut lines, table_start, "model", &format!("\"{escaped}\""));
    }
    if let Some(api_keys) = patch.api_keys.as_ref() {
        let value = render_toml_string_list(api_keys);
        upsert_toml_llm_key(&mut lines, table_start, "api_keys", &value);
    }

    join_lines(&lines, newline, trailing_newline)
}

fn upsert_toml_llm_key(lines: &mut Vec<String>, table_start: usize, key: &str, value: &str) {
    let table_end = find_toml_table_end(lines, table_start);
    let existing = (table_start + 1..table_end).find(|&idx| {
        let trimmed = lines[idx].trim_start();
        trimmed.starts_with(key) && trimmed.contains('=')
    });
    let rendered = format!("{key} = {value}");
    if let Some(index) = existing {
        lines[index] = rendered;
    } else {
        lines.splice(table_end..table_end, vec![rendered]);
    }
}

fn render_toml_string_list(entries: &[String]) -> String {
    if entries.is_empty() {
        return "[]".to_string();
    }
    let rendered = entries
        .iter()
        .map(|entry| {
            let escaped = entry.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

fn find_toml_table_end(lines: &[String], table_start: usize) -> usize {
    for (idx, line) in lines.iter().enumerate().skip(table_start + 1) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return idx;
        }
    }
    lines.len()
}

fn render_toml_whitelist(entries: &[String]) -> String {
    if entries.is_empty() {
        return "whitelist = []".to_string();
    }
    let rendered = entries
        .iter()
        .map(|entry| {
            let escaped = entry.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("whitelist = [{rendered}]")
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

fn detect_newline(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn join_lines(lines: &[String], newline: &str, trailing_newline: bool) -> String {
    let mut rendered = lines.join(newline);
    if trailing_newline || !rendered.ends_with(newline) {
        rendered.push_str(newline);
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_yaml_keeps_section_and_rewrites_whitelist() {
        let source = "onebot-v11:\n  whitelist: []\nrust:\n  adapters: []\n";
        let updated =
            update_yaml_document(source, &["private:1".to_string(), "group:2".to_string()]);
        assert!(
            updated.contains("onebot-v11:\n  whitelist:\n    - 'private:1'\n    - 'group:2'\n")
        );
        assert!(updated.contains("rust:\n  adapters: []"));
    }

    #[test]
    fn update_yaml_inserts_section_when_missing() {
        let source = "rust:\n  adapters: []\n";
        let updated = update_yaml_document(source, &["private:42".to_string()]);
        assert!(updated.contains("onebot-v11:\n  whitelist:\n    - 'private:42'"));
    }

    #[test]
    fn update_toml_rewrites_existing_table() {
        let source = "[onebot-v11]\nwhitelist = []\n\n[rust]\nadapters = []\n";
        let updated = update_toml_document(
            source,
            &["private:1000".to_string(), "group:2000".to_string()],
        );
        assert!(updated.contains("whitelist = [\"private:1000\", \"group:2000\"]"));
        assert!(updated.contains("[rust]"));
    }

    #[test]
    fn update_yaml_llm_rewrites_target_fields() {
        let source = "llm:\n  enabled: false\n  provider: 'openai'\n  model: 'gpt-old'\n";
        let patch = LlmConfigPatch {
            enabled: Some(true),
            provider: Some("openai".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            api_keys: Some(vec!["k1".to_string(), "k2".to_string()]),
        };
        let updated = update_yaml_llm_document(source, &patch);
        assert!(updated.contains("llm:"));
        assert!(updated.contains("enabled: true"));
        assert!(updated.contains("provider: 'openai'"));
        assert!(updated.contains("model: 'gpt-4.1-mini'"));
        assert!(updated.contains("api_keys:\n    - 'k1'\n    - 'k2'"));
    }

    #[test]
    fn update_toml_llm_inserts_section_when_missing() {
        let source = "[rust]\nadapters = []\n";
        let patch = LlmConfigPatch {
            enabled: Some(true),
            provider: Some("openai".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            api_keys: Some(vec!["k1".to_string()]),
        };
        let updated = update_toml_llm_document(source, &patch);
        assert!(updated.contains("[llm]"));
        assert!(updated.contains("enabled = true"));
        assert!(updated.contains("provider = \"openai\""));
        assert!(updated.contains("model = \"gpt-4.1-mini\""));
        assert!(updated.contains("api_keys = [\"k1\"]"));
    }
}
