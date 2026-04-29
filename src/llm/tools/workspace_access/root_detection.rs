use std::fs;
use std::path::{Path, PathBuf};

pub(in super::super) fn resolve_workspace_root() -> Result<PathBuf, String> {
    if let Some(explicit_root) = std::env::var_os("LY_WORKSPACE_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Ok(fs::canonicalize(&explicit_root).unwrap_or(explicit_root));
    }

    let current = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current workspace: {err}"))?;
    let canonical = fs::canonicalize(&current).unwrap_or(current);
    Ok(detect_workspace_root(canonical.as_path()))
}

fn detect_workspace_root(start: &Path) -> PathBuf {
    let mut ancestors_within_git_boundary = Vec::new();
    let mut stopped_at_git_boundary = false;

    for ancestor in start.ancestors() {
        ancestors_within_git_boundary.push(ancestor);
        if ancestor.join(".git").exists() {
            stopped_at_git_boundary = true;
            break;
        }
    }

    if let Some(workspace_root) = ancestors_within_git_boundary.iter().find_map(|ancestor| {
        cargo_manifest_is_workspace(ancestor.join("Cargo.toml").as_path())
            .filter(|is_workspace_manifest| *is_workspace_manifest)
            .map(|_| (*ancestor).to_path_buf())
    }) {
        return workspace_root;
    }
    if stopped_at_git_boundary {
        return ancestors_within_git_boundary
            .last()
            .map(|ancestor| (*ancestor).to_path_buf())
            .unwrap_or_else(|| start.to_path_buf());
    }

    start
        .ancestors()
        .filter(|ancestor| {
            cargo_manifest_is_workspace(ancestor.join("Cargo.toml").as_path()).is_some()
        })
        .map(Path::to_path_buf)
        .last()
        .unwrap_or_else(|| start.to_path_buf())
}

fn cargo_manifest_is_workspace(path: &Path) -> Option<bool> {
    if !path.is_file() {
        return None;
    }

    Some(fs::read_to_string(path).ok().is_some_and(|content| {
        content
            .lines()
            .any(|line| line.split('#').next().unwrap_or("").trim() == "[workspace]")
    }))
}

#[cfg(test)]
#[path = "root_detection/tests.rs"]
mod tests;
