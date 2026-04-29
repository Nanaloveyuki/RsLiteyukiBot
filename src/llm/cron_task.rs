#![allow(dead_code)]

#[path = "cron_task/persistence.rs"]
mod persistence;
#[path = "cron_task/schedule.rs"]
mod schedule;

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::utils::config_path::resolve_preferred_plugin_cron_state_path;
use liteyukibot_core::{PluginCapabilitySnapshot, PluginRegisteredCronJob};

use self::persistence::{
    CronTaskRuntimeEntry, cron_state_backup_path, persist_state_document, read_state_document,
};
#[cfg(test)]
use self::schedule::next_cron_occurrence;
use self::schedule::{
    apply_scheduler_overlay, compute_next_run_time, job_schedule_signature, parse_timestamp,
};

pub(crate) use self::schedule::cron_job_is_host_executable;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CronTaskKey {
    pub(crate) plugin_id: String,
    pub(crate) job_id: String,
}

impl CronTaskKey {
    pub(crate) fn new(plugin_id: impl Into<String>, job_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            job_id: job_id.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DueCronJob {
    pub(crate) key: CronTaskKey,
    pub(crate) job: PluginRegisteredCronJob,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PluginCronTaskScheduler {
    path: PathBuf,
    entries: BTreeMap<CronTaskKey, CronTaskRuntimeEntry>,
    warnings: Vec<String>,
    load_error: Option<String>,
}

impl PluginCronTaskScheduler {
    pub(crate) fn from_default_path() -> Self {
        Self::from_config_path(resolve_preferred_plugin_cron_state_path())
    }

    pub(crate) fn from_config_path(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let backup_path = cron_state_backup_path(path.as_path());
        let mut warnings = Vec::new();
        match read_state_document(path.as_path(), backup_path.as_path()) {
            Ok((document, source_warning)) => {
                if let Some(source_warning) = source_warning {
                    warnings.push(source_warning);
                }
                let entries = document.into_runtime_entries();
                Self {
                    path,
                    entries,
                    warnings,
                    load_error: None,
                }
            }
            Err(err) => Self {
                path,
                entries: BTreeMap::new(),
                warnings: vec![err.clone()],
                load_error: None,
            },
        }
    }

    pub(crate) fn warnings(&self) -> &[String] {
        self.warnings.as_slice()
    }

    pub(crate) fn sync_snapshot(
        &mut self,
        snapshot: &mut PluginCapabilitySnapshot,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.sync_snapshots(std::slice::from_mut(snapshot), false, now)
    }

    pub(crate) fn sync_snapshots(
        &mut self,
        snapshots: &mut [PluginCapabilitySnapshot],
        prune_missing: bool,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.ensure_loaded()?;
        let mut seen = HashSet::new();
        for snapshot in snapshots {
            for job in &mut snapshot.cron_jobs {
                let key = CronTaskKey::new(snapshot.plugin_id.clone(), job.job_id.clone());
                seen.insert(key.clone());
                let entry = self.entries.entry(key).or_default();
                apply_scheduler_overlay(job, entry, now);
            }
        }

        if prune_missing {
            self.entries.retain(|key, _| seen.contains(key));
        }
        self.persist()
    }

    pub(crate) fn collect_due_jobs(
        &self,
        snapshots: &[PluginCapabilitySnapshot],
        disabled_plugin_ids: &HashSet<String>,
        now: DateTime<Utc>,
    ) -> Vec<DueCronJob> {
        snapshots
            .iter()
            .filter(|snapshot| !disabled_plugin_ids.contains(snapshot.plugin_id.as_str()))
            .flat_map(|snapshot| {
                snapshot
                    .cron_jobs
                    .iter()
                    .filter(move |job| {
                        cron_job_is_host_executable(job)
                            && job.enabled
                            && job
                                .next_run_time
                                .as_deref()
                                .and_then(parse_timestamp)
                                .is_some_and(|scheduled| scheduled <= now)
                    })
                    .cloned()
                    .map(move |job| DueCronJob {
                        key: CronTaskKey::new(snapshot.plugin_id.clone(), job.job_id.clone()),
                        job,
                    })
            })
            .collect()
    }

    pub(crate) fn mark_job_success(
        &mut self,
        key: &CronTaskKey,
        job: &PluginRegisteredCronJob,
        ran_at: DateTime<Utc>,
    ) -> Result<(), String> {
        self.ensure_loaded()?;
        let entry = self.entries.entry(key.clone()).or_default();
        entry.last_run_time = Some(ran_at.to_rfc3339());
        entry.last_error = None;
        entry.schedule_signature = Some(job_schedule_signature(job));
        let mut overlay_job = job.clone();
        overlay_job.last_run_time = entry.last_run_time.clone();
        overlay_job.last_error = None;
        overlay_job.next_run_time = entry.next_run_time.clone();
        apply_scheduler_overlay(&mut overlay_job, entry, ran_at);
        entry.next_run_time = overlay_job.next_run_time.clone();
        self.persist()
    }

    pub(crate) fn mark_job_error(
        &mut self,
        key: &CronTaskKey,
        job: &PluginRegisteredCronJob,
        ran_at: DateTime<Utc>,
        error: impl Into<String>,
    ) -> Result<(), String> {
        self.ensure_loaded()?;
        let entry = self.entries.entry(key.clone()).or_default();
        entry.last_run_time = Some(ran_at.to_rfc3339());
        entry.last_error = Some(error.into());
        entry.schedule_signature = Some(job_schedule_signature(job));
        if job.run_once {
            entry.next_run_time = None;
        } else {
            entry.next_run_time = compute_next_run_time(job, Some(ran_at), ran_at)
                .map_err(|err| format!("failed to compute next run for '{}': {err}", job.job_id))?;
        }
        self.persist()
    }

    pub(crate) fn plugin_has_executable_jobs(&self, snapshot: &PluginCapabilitySnapshot) -> bool {
        snapshot
            .cron_jobs
            .iter()
            .any(|job| cron_job_is_host_executable(job) && job.enabled)
    }

    pub(crate) fn plugin_scheduler_status(&self, snapshot: &PluginCapabilitySnapshot) -> String {
        if snapshot.cron_jobs.is_empty() {
            return "unsupported".to_string();
        }
        if snapshot.cron_jobs.iter().all(|job| !job.enabled) {
            return "disabled".to_string();
        }
        let executable_jobs = snapshot
            .cron_jobs
            .iter()
            .filter(|job| cron_job_is_host_executable(job) && job.enabled)
            .collect::<Vec<_>>();
        if executable_jobs.is_empty() {
            return "registered_only".to_string();
        }
        if executable_jobs.iter().any(|job| job.last_error.is_some()) {
            return "error".to_string();
        }
        "active".to_string()
    }

    fn ensure_loaded(&self) -> Result<(), String> {
        if let Some(err) = &self.load_error {
            Err(err.clone())
        } else {
            Ok(())
        }
    }

    fn persist(&self) -> Result<(), String> {
        persist_state_document(self.path.as_path(), &self.entries)
    }
}

#[cfg(test)]
#[path = "cron_task/tests.rs"]
mod tests;
