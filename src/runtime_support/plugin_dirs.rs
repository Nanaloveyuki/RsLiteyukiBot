use super::*;

fn resolve_user_home_dir() -> Option<PathBuf> {
    crate::utils::config_path::resolve_user_home_dir()
}

pub(crate) fn resolve_local_plugin_dir() -> PathBuf {
    resolve_user_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".liteyuki")
        .join("plugins")
}

pub(crate) fn resolve_builtin_plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(raw) = std::env::var("LY_PLUGIN_DIRS") {
        for path in std::env::split_paths(&raw) {
            push_explicit_plugin_dir_candidates(&mut dirs, &mut seen, path.as_path());
        }
    }

    push_unique_plugin_path(&mut dirs, &mut seen, resolve_local_plugin_dir());

    if let Ok(current_dir) = std::env::current_dir() {
        push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, current_dir.as_path(), true);
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent()
    {
        push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, parent, false);
    }

    dirs
}

pub(crate) fn push_explicit_plugin_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    path: &std::path::Path,
) {
    push_unique_plugin_path(dirs, seen, path.to_path_buf());
    push_runtime_plugin_dir_candidates(dirs, seen, path, true);
}

pub(crate) fn push_runtime_plugin_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    root: &std::path::Path,
    include_dev_fallback: bool,
) {
    for candidate in BUILTIN_PLUGIN_DIRS {
        push_unique_plugin_path(dirs, seen, root.join(candidate));
    }
    if include_dev_fallback {
        for candidate in DEV_BUILTIN_PLUGIN_DIRS {
            push_unique_plugin_path(dirs, seen, root.join(candidate));
        }
    }
}

fn push_unique_plugin_path(dirs: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        dirs.push(path);
    }
}
