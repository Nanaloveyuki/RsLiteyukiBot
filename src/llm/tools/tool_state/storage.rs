use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct ToolStateDocument {
    #[serde(default)]
    pub(super) tools: BTreeMap<String, bool>,
}

pub(super) fn read_tool_state_source(
    path: &Path,
    backup_path: &Path,
) -> Result<(ToolStateDocument, Option<String>), String> {
    match read_tool_state_document(path) {
        Ok(Some(document)) => Ok((document, None)),
        Ok(None) => match read_tool_state_document(backup_path) {
            Ok(Some(document)) => Ok((
                document,
                Some(format!(
                    "tool state file '{}' was missing; restored state from backup '{}'",
                    path.display(),
                    backup_path.display()
                )),
            )),
            Ok(None) => Ok((ToolStateDocument::default(), None)),
            Err(backup_err) => Err(format!(
                "tool state file '{}' was missing and backup '{}' could not be restored: {backup_err}",
                path.display(),
                backup_path.display()
            )),
        },
        Err(primary_err) => match read_tool_state_document(backup_path) {
            Ok(Some(document)) => Ok((
                document,
                Some(format!(
                    "tool state file '{}' was invalid; restored state from backup '{}'",
                    path.display(),
                    backup_path.display()
                )),
            )),
            Ok(None) => Err(primary_err),
            Err(backup_err) => Err(format!(
                "{primary_err}; backup '{}' could not be restored: {backup_err}",
                backup_path.display()
            )),
        },
    }
}

pub(super) fn persist_tool_state(
    path: &Path,
    tools: &BTreeMap<String, bool>,
) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create tool state directory '{}': {err}",
                parent.display()
            )
        })?;
    }

    let content = serde_json::to_string_pretty(&ToolStateDocument {
        tools: tools.clone(),
    })
    .map_err(|err| format!("failed to serialize tool state: {err}"))?;
    let temp_path = tool_state_temp_path(path);
    let backup_path = tool_state_backup_path(path);
    let mut file = fs::File::create(&temp_path).map_err(|err| {
        format!(
            "failed to create tool state temp file '{}': {err}",
            temp_path.display()
        )
    })?;
    file.write_all(format!("{content}\n").as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            format!(
                "failed to flush tool state temp file '{}': {err}",
                temp_path.display()
            )
        })?;
    drop(file);

    if backup_path.exists() {
        let _ = fs::remove_file(&backup_path);
    }
    if path.exists() {
        fs::rename(path, &backup_path).map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            format!(
                "failed to stage previous tool state file '{}' for replacement: {err}",
                path.display()
            )
        })?;
    }
    if let Err(err) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        if backup_path.exists() {
            let _ = fs::rename(&backup_path, path);
        }
        return Err(format!(
            "failed to replace tool state file '{}': {err}",
            path.display()
        ));
    }
    if backup_path.exists() {
        let _ = fs::remove_file(backup_path);
    }
    Ok(())
}

fn read_tool_state_document(path: &Path) -> Result<Option<ToolStateDocument>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!(
                "tool state file '{}' could not be read: {err}",
                path.display()
            ));
        }
    };

    if content.trim().is_empty() {
        return Ok(Some(ToolStateDocument::default()));
    }

    serde_json::from_str::<ToolStateDocument>(&content)
        .map(Some)
        .map_err(|err| {
            format!(
                "tool state file '{}' is invalid json: {err}",
                path.display()
            )
        })
}

fn tool_state_temp_path(path: &Path) -> PathBuf {
    let suffix = format!("{}.tmp", std::process::id());
    path.with_extension(suffix)
}

pub(in super::super) fn tool_state_backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

#[cfg(test)]
#[path = "storage/tests.rs"]
mod tests;
