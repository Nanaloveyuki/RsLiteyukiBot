use std::path::{Component, Path, PathBuf};

use crate::utils::config_path::resolve_user_config_dir;

pub(super) fn resolve_source_root(plugin_root: &Path, raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_start_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err("source.path should not be empty".to_string());
    }

    let mut relative = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(segment) => relative.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err("source.path escapes plugin root".to_string());
            }
        }
    }
    Ok(plugin_root.join(relative))
}

pub(super) fn path_to_forward_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn default_plugin_config_path(plugin_id: &str) -> PathBuf {
    resolve_user_config_dir()
        .join("plugins")
        .join(format!("{plugin_id}.json"))
}
