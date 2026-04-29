use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};

use crate::llm::cron_task::CronTaskKey;
use crate::plugin::sdk::python::execution::{
    PythonCronExecutionOutcome, execute_python_registered_cron_job,
};

use super::{PluginSdk, PluginSdkError};

impl PluginSdk {
    pub fn run_due_plugin_jobs(
        &self,
        disabled_plugin_ids: &[String],
        now: Option<DateTime<Utc>>,
    ) -> Result<usize, PluginSdkError> {
        let now = now.unwrap_or_else(Utc::now);
        let mut snapshots = self.list_all_plugin_capabilities_raw()?;
        self.sync_all_plugin_cron_snapshots(snapshots.as_mut_slice(), true, now)?;
        let disabled = disabled_plugin_ids.iter().cloned().collect::<HashSet<_>>();
        let due_jobs = {
            let scheduler = self.cron_scheduler.lock().map_err(|_| {
                PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
            })?;
            scheduler.collect_due_jobs(snapshots.as_slice(), &disabled, now)
        };

        if due_jobs.is_empty() {
            return Ok(0);
        }

        let job_map = snapshots
            .iter()
            .flat_map(|snapshot| {
                snapshot.cron_jobs.iter().cloned().map(|job| {
                    (
                        CronTaskKey::new(snapshot.plugin_id.clone(), job.job_id.clone()),
                        job,
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();

        let mut executed = 0usize;
        for due_job in due_jobs {
            let Some(job) = job_map.get(&due_job.key) else {
                continue;
            };
            match execute_python_registered_cron_job(
                &self.python_runtime,
                due_job.key.plugin_id.as_str(),
                due_job.key.job_id.as_str(),
                &due_job.job.payload,
            ) {
                Ok(PythonCronExecutionOutcome::Executed) => {
                    executed = executed.saturating_add(1);
                    self.cron_scheduler
                        .lock()
                        .map_err(|_| {
                            PluginSdkError::Runtime(
                                "plugin cron scheduler lock poisoned".to_string(),
                            )
                        })?
                        .mark_job_success(&due_job.key, job, now)
                        .map_err(PluginSdkError::Runtime)?;
                }
                Ok(PythonCronExecutionOutcome::HandlerMissing) => {
                    self.cron_scheduler
                        .lock()
                        .map_err(|_| {
                            PluginSdkError::Runtime(
                                "plugin cron scheduler lock poisoned".to_string(),
                            )
                        })?
                        .mark_job_error(
                            &due_job.key,
                            job,
                            now,
                            format!(
                                "plugin cron job '{}' does not expose an executable handler",
                                due_job.key.job_id
                            ),
                        )
                        .map_err(PluginSdkError::Runtime)?;
                }
                Ok(PythonCronExecutionOutcome::PluginUnavailable) => {
                    self.cron_scheduler
                        .lock()
                        .map_err(|_| {
                            PluginSdkError::Runtime(
                                "plugin cron scheduler lock poisoned".to_string(),
                            )
                        })?
                        .mark_job_error(
                            &due_job.key,
                            job,
                            now,
                            format!(
                                "plugin '{}' runtime is unavailable for cron job '{}'",
                                due_job.key.plugin_id, due_job.key.job_id
                            ),
                        )
                        .map_err(PluginSdkError::Runtime)?;
                }
                Ok(PythonCronExecutionOutcome::JobNotFound) => {
                    self.cron_scheduler
                        .lock()
                        .map_err(|_| {
                            PluginSdkError::Runtime(
                                "plugin cron scheduler lock poisoned".to_string(),
                            )
                        })?
                        .mark_job_error(
                            &due_job.key,
                            job,
                            now,
                            format!(
                                "plugin cron job '{}' is not registered in runtime for plugin '{}'",
                                due_job.key.job_id, due_job.key.plugin_id
                            ),
                        )
                        .map_err(PluginSdkError::Runtime)?;
                }
                Err(err) => {
                    self.cron_scheduler
                        .lock()
                        .map_err(|_| {
                            PluginSdkError::Runtime(
                                "plugin cron scheduler lock poisoned".to_string(),
                            )
                        })?
                        .mark_job_error(&due_job.key, job, now, err.to_string())
                        .map_err(PluginSdkError::Runtime)?;
                }
            }
        }

        Ok(executed)
    }

    pub fn plugin_cron_scheduler_status(&self, plugin_id: &str) -> Result<String, PluginSdkError> {
        let Some(mut snapshot) = self.get_plugin_capabilities_raw(plugin_id)? else {
            return Ok("unsupported".to_string());
        };
        self.sync_plugin_cron_snapshot(&mut snapshot, Utc::now())?;
        let scheduler = self.cron_scheduler.lock().map_err(|_| {
            PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
        })?;
        Ok(scheduler.plugin_scheduler_status(&snapshot))
    }

    pub fn plugin_has_executable_cron_jobs(&self, plugin_id: &str) -> Result<bool, PluginSdkError> {
        let Some(mut snapshot) = self.get_plugin_capabilities_raw(plugin_id)? else {
            return Ok(false);
        };
        self.sync_plugin_cron_snapshot(&mut snapshot, Utc::now())?;
        let scheduler = self.cron_scheduler.lock().map_err(|_| {
            PluginSdkError::Runtime("plugin cron scheduler lock poisoned".to_string())
        })?;
        Ok(scheduler.plugin_has_executable_jobs(&snapshot))
    }
}
