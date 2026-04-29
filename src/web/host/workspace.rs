use super::*;

#[derive(Debug, Clone, Serialize)]
pub(super) struct WorkspaceFileInfo {
    pub(super) name: String,
    #[serde(rename = "isDirectory")]
    pub(super) is_directory: bool,
    pub(super) size: u64,
    pub(super) mtime: String,
}

pub(super) fn workspace_root() -> PathBuf {
    std::env::var_os("LY_WORKSPACE_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .map(|path| fs::canonicalize(&path).unwrap_or(path))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn sanitize_workspace_relative_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_start_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }

    let mut output = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(segment) => output.push(segment),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("path escapes workspace root".to_string());
            }
        }
    }
    Ok(output)
}

pub(super) fn resolve_workspace_path(raw: &str) -> Result<PathBuf, String> {
    Ok(workspace_root().join(sanitize_workspace_relative_path(raw)?.as_path()))
}

pub(super) fn ensure_path_within_workspace(path: &Path) -> Result<(), String> {
    let root = workspace_root();
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if canonical.starts_with(&root) || path.starts_with(&root) {
        Ok(())
    } else {
        Err("path escapes workspace root".to_string())
    }
}

pub(super) fn parent_or_self(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
}

pub(super) fn build_workspace_file_info(path: &Path) -> Result<WorkspaceFileInfo, String> {
    let metadata =
        fs::metadata(path).map_err(|err| format!("failed to stat {}: {err}", path.display()))?;
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let modified: DateTime<Utc> = modified.into();

    Ok(WorkspaceFileInfo {
        name: path
            .file_name()
            .and_then(|segment| segment.to_str())
            .unwrap_or_default()
            .to_string(),
        is_directory: metadata.is_dir(),
        size: if metadata.is_file() {
            metadata.len()
        } else {
            0
        },
        mtime: modified.to_rfc3339(),
    })
}

pub(super) fn format_log_history(limit: usize) -> String {
    recent_buffered_logs(limit)
        .into_iter()
        .map(|entry| entry.line)
        .collect::<Vec<_>>()
        .join("\n")
}
