use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::llm::client::LlmClientError;

pub(super) fn resolve_workspace_path(
    workspace_root: &Path,
    raw_path: &str,
) -> Result<PathBuf, LlmClientError> {
    let sanitized = sanitize_relative_path(raw_path).map_err(LlmClientError::Tool)?;
    Ok(workspace_root.join(sanitized))
}

pub(super) fn ensure_resolved_path_within_workspace(
    workspace_root: &Path,
    path: &Path,
) -> Result<(), String> {
    if path_is_within_workspace(workspace_root, path) {
        Ok(())
    } else {
        Err(format!(
            "workspace path '{}' escapes the workspace root",
            display_path(path, workspace_root)
        ))
    }
}

pub(super) fn path_is_within_workspace(workspace_root: &Path, path: &Path) -> bool {
    let canonical_root =
        fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    let mut cursor = path;

    loop {
        match fs::canonicalize(cursor) {
            Ok(existing_path) => return existing_path.starts_with(&canonical_root),
            Err(_) => {
                let Some(parent) = cursor.parent() else {
                    return false;
                };
                cursor = parent;
            }
        }
    }
}

fn sanitize_relative_path(raw_path: &str) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("path cannot be empty".to_string());
    }

    let input = Path::new(trimmed);
    if input.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }

    let mut sanitized = PathBuf::new();
    for component in input.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => sanitized.push(part),
            Component::ParentDir => {
                return Err("parent-directory segments are not allowed".to_string());
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err("absolute paths are not allowed".to_string());
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(sanitized)
    }
}

pub(super) fn display_path(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

#[cfg(test)]
#[path = "path_safety/tests.rs"]
mod tests;
