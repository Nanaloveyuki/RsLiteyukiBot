use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::CronTaskKey;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct CronTaskStateDocument {
    #[serde(default)]
    jobs: Vec<CronTaskStateEntry>,
}

impl CronTaskStateDocument {
    pub(super) fn into_runtime_entries(self) -> BTreeMap<CronTaskKey, CronTaskRuntimeEntry> {
        self.jobs
            .into_iter()
            .map(|entry| {
                (
                    CronTaskKey::new(entry.plugin_id.clone(), entry.job_id.clone()),
                    entry.into(),
                )
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CronTaskStateEntry {
    plugin_id: String,
    job_id: String,
    #[serde(default)]
    schedule_signature: Option<String>,
    #[serde(default)]
    next_run_time: Option<String>,
    #[serde(default)]
    last_run_time: Option<String>,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct CronTaskRuntimeEntry {
    pub(super) schedule_signature: Option<String>,
    pub(super) next_run_time: Option<String>,
    pub(super) last_run_time: Option<String>,
    pub(super) last_error: Option<String>,
}

impl From<CronTaskStateEntry> for CronTaskRuntimeEntry {
    fn from(value: CronTaskStateEntry) -> Self {
        Self {
            schedule_signature: value.schedule_signature,
            next_run_time: value.next_run_time,
            last_run_time: value.last_run_time,
            last_error: value.last_error,
        }
    }
}

pub(super) fn read_state_document(
    path: &Path,
    backup_path: &Path,
) -> Result<(CronTaskStateDocument, Option<String>), String> {
    match read_state_document_path(path) {
        Ok(Some(document)) => return Ok((document, None)),
        Ok(None) => {}
        Err(primary_err) => {
            if let Ok(Some(document)) = read_state_document_path(backup_path) {
                return Ok((
                    document,
                    Some(format!(
                        "plugin cron state file '{}' was invalid; restored state from backup '{}'",
                        path.display(),
                        backup_path.display()
                    )),
                ));
            }
            return Err(primary_err);
        }
    }

    if let Ok(Some(document)) = read_state_document_path(backup_path) {
        return Ok((
            document,
            Some(format!(
                "plugin cron state file '{}' was missing; restored state from backup '{}'",
                path.display(),
                backup_path.display()
            )),
        ));
    }

    Ok((CronTaskStateDocument::default(), None))
}

pub(super) fn persist_state_document(
    path: &Path,
    entries: &BTreeMap<CronTaskKey, CronTaskRuntimeEntry>,
) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create plugin cron state directory '{}': {err}",
                parent.display()
            )
        })?;
    }

    let document = CronTaskStateDocument {
        jobs: entries
            .iter()
            .map(|(key, entry)| CronTaskStateEntry {
                plugin_id: key.plugin_id.clone(),
                job_id: key.job_id.clone(),
                schedule_signature: entry.schedule_signature.clone(),
                next_run_time: entry.next_run_time.clone(),
                last_run_time: entry.last_run_time.clone(),
                last_error: entry.last_error.clone(),
            })
            .collect(),
    };
    let content = serde_json::to_string_pretty(&document)
        .map_err(|err| format!("failed to serialize plugin cron state: {err}"))?;
    let temp_path = cron_state_temp_path(path);
    let backup_path = cron_state_backup_path(path);
    let mut file = fs::File::create(&temp_path).map_err(|err| {
        format!(
            "failed to create plugin cron state temp file '{}': {err}",
            temp_path.display()
        )
    })?;
    file.write_all(format!("{content}\n").as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            format!(
                "failed to flush plugin cron state temp file '{}': {err}",
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
                "failed to stage previous plugin cron state file '{}': {err}",
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
            "failed to replace plugin cron state file '{}': {err}",
            path.display()
        ));
    }
    if backup_path.exists() {
        let _ = fs::remove_file(backup_path);
    }
    Ok(())
}

pub(super) fn cron_state_backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn read_state_document_path(path: &Path) -> Result<Option<CronTaskStateDocument>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!(
                "plugin cron state file '{}' could not be read: {err}",
                path.display()
            ));
        }
    };
    if content.trim().is_empty() {
        return Ok(Some(CronTaskStateDocument::default()));
    }
    serde_json::from_str::<CronTaskStateDocument>(&content)
        .map(Some)
        .map_err(|err| {
            format!(
                "plugin cron state file '{}' is invalid json: {err}",
                path.display()
            )
        })
}

fn cron_state_temp_path(path: &Path) -> PathBuf {
    path.with_extension(format!("json.tmp.{}", std::process::id()))
}
