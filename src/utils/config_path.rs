use std::path::{Path, PathBuf};

use crate::hardcode_data::config_path::{
    APP_CONFIG_FILENAMES, LEGACY_LLM_PROMPT_STORE_PATH, LEGACY_WEBUI_PASSWORD_RELATIVE_PATH,
    LLM_CONFIG_FILENAMES, LLM_PROMPT_STORE_FILENAME, MCP_CONFIG_FILENAME, PASSWORD_CONFIG_FILENAME,
    PLUGIN_CRON_STATE_FILENAME, SKILLS_DIR_NAME, TOOL_STATE_FILENAME, WEBUI_PASSWORD_FILENAME,
};

pub(crate) fn resolve_env_path(key: &str) -> Option<PathBuf> {
    let value = std::env::var(key).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

pub(crate) fn resolve_user_home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

pub(crate) fn resolve_liteyuki_root_dir() -> PathBuf {
    resolve_user_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".liteyuki")
}

pub(crate) fn resolve_user_config_dir() -> PathBuf {
    resolve_liteyuki_root_dir().join("configs")
}

pub(crate) fn resolve_user_skills_dir() -> PathBuf {
    resolve_liteyuki_root_dir().join(SKILLS_DIR_NAME)
}

pub(crate) fn resolve_user_config_file(filename: &str) -> PathBuf {
    resolve_user_config_dir().join(filename)
}

pub(crate) fn resolve_default_app_config_path() -> PathBuf {
    resolve_user_config_file(APP_CONFIG_FILENAMES[0])
}

pub(crate) fn resolve_default_llm_config_path() -> PathBuf {
    resolve_user_config_file(LLM_CONFIG_FILENAMES[0])
}

pub(crate) fn resolve_default_password_config_path() -> PathBuf {
    resolve_user_config_file(PASSWORD_CONFIG_FILENAME)
}

pub(crate) fn resolve_default_webui_password_path() -> PathBuf {
    resolve_user_config_file(WEBUI_PASSWORD_FILENAME)
}

pub(crate) fn resolve_default_llm_prompt_store_path() -> PathBuf {
    resolve_user_config_file(LLM_PROMPT_STORE_FILENAME)
}

pub(crate) fn resolve_default_mcp_config_path() -> PathBuf {
    resolve_user_config_file(MCP_CONFIG_FILENAME)
}

pub(crate) fn resolve_default_tool_state_path() -> PathBuf {
    resolve_user_config_file(TOOL_STATE_FILENAME)
}

pub(crate) fn resolve_default_plugin_cron_state_path() -> PathBuf {
    resolve_user_config_file(PLUGIN_CRON_STATE_FILENAME)
}

pub(crate) fn resolve_existing_named_config_path(
    env_key: &str,
    user_filenames: &[&str],
    legacy_paths: &[&str],
) -> Option<PathBuf> {
    resolve_env_path(env_key).or_else(|| {
        user_filenames
            .iter()
            .map(|filename| resolve_user_config_file(filename))
            .chain(legacy_paths.iter().map(PathBuf::from))
            .find(|path| path.exists())
    })
}

pub(crate) fn resolve_existing_user_named_config_path(user_filenames: &[&str]) -> Option<PathBuf> {
    user_filenames
        .iter()
        .map(|filename| resolve_user_config_file(filename))
        .find(|path| path.exists())
}

fn ensure_parent_dir(target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create config parent directory {}: {err}",
                parent.display()
            )
        })?;
    }
    Ok(())
}

fn move_config_file(source: &Path, target: &Path) -> Result<(), String> {
    ensure_parent_dir(target)?;

    match std::fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            std::fs::copy(source, target).map_err(|copy_err| {
                format!(
                    "failed to migrate config {} -> {}: rename error: {rename_err}; copy error: {copy_err}",
                    source.display(),
                    target.display()
                )
            })?;
            std::fs::remove_file(source).map_err(|remove_err| {
                let _ = std::fs::remove_file(target);
                format!(
                    "failed to remove legacy config {} after copy to {}: {remove_err}",
                    source.display(),
                    target.display()
                )
            })?;
            Ok(())
        }
    }
}

pub(crate) fn replace_config_file(source: &Path, target: &Path) -> Result<(), String> {
    ensure_parent_dir(target)?;

    if target.exists() {
        std::fs::remove_file(target).map_err(|err| {
            format!(
                "failed to remove existing config {} before migrating {}: {err}",
                target.display(),
                source.display()
            )
        })?;
    }

    move_config_file(source, target)
}

fn migration_target_name(
    source_relative_path: &str,
    user_filenames: &[&str],
) -> Result<String, String> {
    if user_filenames.contains(&source_relative_path) {
        return Ok(source_relative_path.to_string());
    }

    Path::new(source_relative_path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .ok_or_else(|| {
            format!(
                "failed to derive migration target name from legacy path {}",
                source_relative_path
            )
        })
}

pub(crate) fn migrate_legacy_named_config_to_user_dir(
    env_key: &str,
    user_filenames: &[&str],
    legacy_paths: &[&str],
) -> Result<Option<PathBuf>, String> {
    if resolve_env_path(env_key).is_some() {
        return Ok(None);
    }
    if resolve_existing_user_named_config_path(user_filenames).is_some() {
        return Ok(None);
    }

    for source_relative_path in legacy_paths {
        let source = PathBuf::from(source_relative_path);
        if !source.exists() {
            continue;
        }

        let target_name = migration_target_name(source_relative_path, user_filenames)?;
        let target = resolve_user_config_file(target_name.as_str());
        if source == target {
            return Ok(Some(target));
        }

        move_config_file(source.as_path(), target.as_path())?;
        return Ok(Some(target));
    }

    Ok(None)
}

pub(crate) fn resolve_preferred_named_config_path(
    env_key: &str,
    user_filenames: &[&str],
    legacy_paths: &[&str],
    default_path: impl FnOnce() -> PathBuf,
) -> PathBuf {
    resolve_existing_named_config_path(env_key, user_filenames, legacy_paths)
        .unwrap_or_else(default_path)
}

pub(crate) fn resolve_existing_app_config_path() -> Option<PathBuf> {
    resolve_existing_named_config_path(
        "LY_CONFIG_PATH",
        &APP_CONFIG_FILENAMES,
        &APP_CONFIG_FILENAMES,
    )
}

pub(crate) fn resolve_existing_llm_config_path() -> Option<PathBuf> {
    resolve_existing_named_config_path(
        "LY_LLM_CONFIG_PATH",
        &LLM_CONFIG_FILENAMES,
        &LLM_CONFIG_FILENAMES,
    )
}

pub(crate) fn resolve_existing_legacy_app_config_path() -> Option<PathBuf> {
    APP_CONFIG_FILENAMES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

pub(crate) fn resolve_existing_legacy_llm_config_path() -> Option<PathBuf> {
    LLM_CONFIG_FILENAMES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

pub(crate) fn resolve_existing_user_app_config_path() -> Option<PathBuf> {
    resolve_existing_user_named_config_path(&APP_CONFIG_FILENAMES)
}

pub(crate) fn resolve_existing_user_llm_config_path() -> Option<PathBuf> {
    resolve_existing_user_named_config_path(&LLM_CONFIG_FILENAMES)
}

pub(crate) fn migrate_legacy_app_config_to_user_dir() -> Result<Option<PathBuf>, String> {
    migrate_legacy_named_config_to_user_dir(
        "LY_CONFIG_PATH",
        &APP_CONFIG_FILENAMES,
        &APP_CONFIG_FILENAMES,
    )
}

pub(crate) fn migrate_legacy_llm_config_to_user_dir() -> Result<Option<PathBuf>, String> {
    migrate_legacy_named_config_to_user_dir(
        "LY_LLM_CONFIG_PATH",
        &LLM_CONFIG_FILENAMES,
        &LLM_CONFIG_FILENAMES,
    )
}

pub(crate) fn resolve_preferred_password_config_path() -> PathBuf {
    resolve_preferred_named_config_path(
        "LY_PASSWORD_PATH",
        &[PASSWORD_CONFIG_FILENAME],
        &[PASSWORD_CONFIG_FILENAME],
        resolve_default_password_config_path,
    )
}

pub(crate) fn resolve_preferred_webui_password_path() -> PathBuf {
    resolve_env_path("LY_WEBUI_PASSWORD_PATH")
        .or_else(|| {
            std::iter::once(resolve_user_config_file(WEBUI_PASSWORD_FILENAME))
                .chain(legacy_webui_password_candidates())
                .find(|path| path.exists())
        })
        .unwrap_or_else(resolve_default_webui_password_path)
}

pub(crate) fn resolve_preferred_llm_prompt_store_path() -> PathBuf {
    resolve_preferred_named_config_path(
        "LY_LLM_PROMPT_STORE_PATH",
        &[LLM_PROMPT_STORE_FILENAME],
        &[LEGACY_LLM_PROMPT_STORE_PATH],
        resolve_default_llm_prompt_store_path,
    )
}

pub(crate) fn resolve_preferred_mcp_config_path() -> PathBuf {
    resolve_preferred_named_config_path(
        "LY_MCP_CONFIG_PATH",
        &[MCP_CONFIG_FILENAME],
        &[MCP_CONFIG_FILENAME],
        resolve_default_mcp_config_path,
    )
}

pub(crate) fn resolve_preferred_tool_state_path() -> PathBuf {
    resolve_preferred_named_config_path(
        "LY_TOOL_STATE_PATH",
        &[TOOL_STATE_FILENAME],
        &[],
        resolve_default_tool_state_path,
    )
}

pub(crate) fn resolve_preferred_plugin_cron_state_path() -> PathBuf {
    resolve_preferred_named_config_path(
        "LY_PLUGIN_CRON_STATE_PATH",
        &[PLUGIN_CRON_STATE_FILENAME],
        &[],
        resolve_default_plugin_cron_state_path,
    )
}

fn legacy_webui_password_candidates() -> impl Iterator<Item = PathBuf> {
    resolve_user_home_dir()
        .map(|home| home.join(".liteyuki").join(WEBUI_PASSWORD_FILENAME))
        .into_iter()
        .chain(std::iter::once(PathBuf::from(
            LEGACY_WEBUI_PASSWORD_RELATIVE_PATH,
        )))
}
