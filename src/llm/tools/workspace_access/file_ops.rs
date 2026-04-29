use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::llm::client::{LlmClientError, LlmToolOutput};

use super::super::tool_arguments::{optional_string, optional_usize, required_string};
use super::super::{
    DEFAULT_FILE_LIST_MAX_DEPTH, DEFAULT_FILE_LIST_MAX_ENTRIES, DEFAULT_FILE_READ_MAX_CHARS,
    MAX_FILE_LIST_MAX_DEPTH, MAX_FILE_LIST_MAX_ENTRIES, MAX_FILE_READ_MAX_CHARS,
};
use super::path_safety::{
    display_path, ensure_resolved_path_within_workspace, path_is_within_workspace,
    resolve_workspace_path,
};

pub(in super::super) fn list_workspace_files(
    workspace_root: &Path,
    arguments: &Value,
) -> Result<LlmToolOutput, LlmClientError> {
    let path = optional_string(arguments, "path")?
        .map(|value| resolve_workspace_path(workspace_root, value.as_str()))
        .transpose()?
        .unwrap_or_else(|| workspace_root.to_path_buf());
    let max_depth = optional_usize(arguments, "max_depth")?
        .unwrap_or(DEFAULT_FILE_LIST_MAX_DEPTH)
        .clamp(0, MAX_FILE_LIST_MAX_DEPTH);
    let max_entries = optional_usize(arguments, "max_entries")?
        .unwrap_or(DEFAULT_FILE_LIST_MAX_ENTRIES)
        .clamp(1, MAX_FILE_LIST_MAX_ENTRIES);
    ensure_resolved_path_within_workspace(workspace_root, path.as_path())
        .map_err(LlmClientError::Tool)?;

    let mut files = Vec::new();
    collect_workspace_files(
        workspace_root,
        path.as_path(),
        0,
        max_depth,
        max_entries,
        &mut files,
    )
    .map_err(LlmClientError::Tool)?;

    Ok(LlmToolOutput::Json(json!({
        "root": display_path(path.as_path(), workspace_root),
        "files": files,
        "count": files.len(),
    })))
}

pub(in super::super) fn read_workspace_file(
    workspace_root: &Path,
    arguments: &Value,
) -> Result<LlmToolOutput, LlmClientError> {
    let path =
        resolve_workspace_path(workspace_root, required_string(arguments, "path")?.as_str())?;
    ensure_resolved_path_within_workspace(workspace_root, path.as_path())
        .map_err(LlmClientError::Tool)?;
    if !path.is_file() {
        return Err(LlmClientError::Tool(format!(
            "workspace path '{}' is not a file",
            display_path(path.as_path(), workspace_root)
        )));
    }

    let max_chars = optional_usize(arguments, "max_chars")?
        .unwrap_or(DEFAULT_FILE_READ_MAX_CHARS)
        .clamp(256, MAX_FILE_READ_MAX_CHARS);
    let start_line = optional_usize(arguments, "start_line")?.unwrap_or(1).max(1);
    let content_bytes = fs::read(&path).map_err(|err| {
        LlmClientError::Tool(format!(
            "failed to read file '{}': {err}",
            display_path(path.as_path(), workspace_root)
        ))
    })?;
    let mut rendered = format!("File: {}\n", display_path(path.as_path(), workspace_root));
    let text = String::from_utf8_lossy(&content_bytes).into_owned();
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        rendered.push_str("[empty file]");
        return Ok(LlmToolOutput::Text(rendered));
    }
    if start_line > lines.len() {
        rendered.push_str("[requested line range is empty]");
        return Ok(LlmToolOutput::Text(rendered));
    }

    let total_lines = lines.len();
    let end_line = optional_usize(arguments, "end_line")?
        .unwrap_or(total_lines)
        .clamp(start_line, total_lines);
    let start_index = start_line.saturating_sub(1);
    let end_index = end_line;

    let mut truncated = false;
    for (offset, line) in lines[start_index..end_index].iter().enumerate() {
        let line_no = start_index + offset + 1;
        let next = format!("{line_no:>4} | {line}\n");
        if rendered.chars().count() + next.chars().count() > max_chars {
            truncated = true;
            break;
        }
        rendered.push_str(next.as_str());
    }
    if truncated {
        rendered.push_str("[truncated]\n");
    }
    Ok(LlmToolOutput::Text(rendered.trim_end().to_string()))
}

fn collect_workspace_files(
    workspace_root: &Path,
    path: &Path,
    depth: usize,
    max_depth: usize,
    max_entries: usize,
    output: &mut Vec<String>,
) -> Result<(), String> {
    if output.len() >= max_entries {
        return Ok(());
    }
    if path.is_file() {
        output.push(display_path(path, workspace_root));
        return Ok(());
    }
    if !path.is_dir() {
        return Err(format!(
            "workspace path '{}' is not a file or directory",
            display_path(path, workspace_root)
        ));
    }

    let mut entries = fs::read_dir(path)
        .map_err(|err| format!("failed to read directory '{}': {err}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("failed to enumerate '{}': {err}", path.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if output.len() >= max_entries {
            break;
        }

        let entry_path = entry.path();
        if !path_is_within_workspace(workspace_root, entry_path.as_path()) {
            continue;
        }
        let Some(name) = entry_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if entry_path.is_dir() {
            if depth >= max_depth
                || super::super::IGNORED_DIR_NAMES
                    .iter()
                    .any(|ignored| ignored == &name)
            {
                continue;
            }
            collect_workspace_files(
                workspace_root,
                entry_path.as_path(),
                depth + 1,
                max_depth,
                max_entries,
                output,
            )?;
        } else if entry_path.is_file() {
            output.push(display_path(entry_path.as_path(), workspace_root));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "file_ops/tests.rs"]
mod tests;
