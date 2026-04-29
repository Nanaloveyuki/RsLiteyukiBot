#[path = "schedule/cron_parser.rs"]
mod cron_parser;
#[path = "schedule/overlay.rs"]
mod overlay;

use chrono::{DateTime, Utc};

use liteyukibot_core::PluginRegisteredCronJob;

use super::persistence::CronTaskRuntimeEntry;

pub(crate) fn cron_job_is_host_executable(job: &PluginRegisteredCronJob) -> bool {
    overlay::cron_job_is_host_executable(job)
}

pub(super) fn apply_scheduler_overlay(
    job: &mut PluginRegisteredCronJob,
    entry: &mut CronTaskRuntimeEntry,
    now: DateTime<Utc>,
) {
    overlay::apply_scheduler_overlay(job, entry, now)
}

pub(super) fn compute_next_run_time(
    job: &PluginRegisteredCronJob,
    last_run_time: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<Option<String>, String> {
    overlay::compute_next_run_time(job, last_run_time, now)
}

pub(super) fn job_schedule_signature(job: &PluginRegisteredCronJob) -> String {
    overlay::job_schedule_signature(job)
}

pub(super) fn parse_timestamp(source: &str) -> Option<DateTime<Utc>> {
    overlay::parse_timestamp(source)
}

#[cfg(test)]
pub(super) fn next_cron_occurrence(
    expression: &str,
    timezone: Option<&str>,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    cron_parser::next_cron_occurrence(expression, timezone, after)
}
