use chrono::{DateTime, Duration, Utc};

use liteyukibot_core::PluginRegisteredCronJob;

use super::super::persistence::CronTaskRuntimeEntry;

const EXECUTABLE_CRON_JOB_TYPES: &[&str] = &["basic"];

pub(super) fn cron_job_is_host_executable(job: &PluginRegisteredCronJob) -> bool {
    job.enabled
        && EXECUTABLE_CRON_JOB_TYPES
            .iter()
            .any(|kind| kind == &job.job_type.as_str())
        && (job.cron_expression.is_some() || job.run_once)
}

pub(super) fn apply_scheduler_overlay(
    job: &mut PluginRegisteredCronJob,
    entry: &mut CronTaskRuntimeEntry,
    now: DateTime<Utc>,
) {
    let schedule_signature = job_schedule_signature(job);
    if entry.last_run_time.is_some() {
        job.last_run_time = entry.last_run_time.clone();
    } else {
        entry.last_run_time = job.last_run_time.clone();
    }
    if entry.last_error.is_some() {
        job.last_error = entry.last_error.clone();
    } else {
        entry.last_error = job.last_error.clone();
    }

    if !cron_job_is_host_executable(job) {
        entry.schedule_signature = Some(schedule_signature);
        entry.next_run_time = None;
        job.next_run_time = None;
        return;
    }

    if job.run_once && entry.last_run_time.is_some() {
        entry.schedule_signature = Some(schedule_signature);
        entry.next_run_time = None;
        job.next_run_time = None;
        return;
    }

    let schedule_changed = entry.schedule_signature.as_deref() != Some(schedule_signature.as_str());
    if !schedule_changed
        && entry
            .next_run_time
            .as_deref()
            .and_then(parse_timestamp)
            .is_some()
    {
        entry.schedule_signature = Some(schedule_signature);
        job.next_run_time = entry.next_run_time.clone();
        return;
    }

    match compute_next_run_time(
        job,
        entry.last_run_time.as_deref().and_then(parse_timestamp),
        now,
    ) {
        Ok(next_run_time) => {
            entry.schedule_signature = Some(schedule_signature);
            entry.next_run_time = next_run_time.clone();
            job.next_run_time = next_run_time;
        }
        Err(err) => {
            entry.schedule_signature = Some(schedule_signature);
            entry.next_run_time = None;
            entry.last_error = Some(err.clone());
            job.next_run_time = None;
            job.last_error = Some(err);
        }
    }
}

pub(super) fn compute_next_run_time(
    job: &PluginRegisteredCronJob,
    last_run_time: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<Option<String>, String> {
    let schedule_reference = last_run_time.unwrap_or_else(|| now - Duration::minutes(1));
    if !job.enabled {
        return Ok(None);
    }
    if job.run_once {
        if last_run_time.is_some() {
            return Ok(None);
        }
        if let Some(expression) = non_empty_cron_expression(job) {
            return super::cron_parser::next_cron_occurrence(
                expression,
                job.timezone.as_deref(),
                schedule_reference,
            )
            .map(|value| Some(value.to_rfc3339()));
        }
        return Ok(Some(now.to_rfc3339()));
    }

    let Some(expression) = non_empty_cron_expression(job) else {
        return Ok(None);
    };
    super::cron_parser::next_cron_occurrence(
        expression,
        job.timezone.as_deref(),
        schedule_reference,
    )
    .map(|value| Some(value.to_rfc3339()))
}

pub(super) fn job_schedule_signature(job: &PluginRegisteredCronJob) -> String {
    let cron_expression = non_empty_cron_expression(job).unwrap_or_default();
    let timezone = job
        .timezone
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    format!(
        "{}|{}|{}|{}|{}",
        job.job_type.trim(),
        cron_expression,
        timezone,
        job.run_once,
        job.enabled
    )
}

pub(super) fn parse_timestamp(source: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(source)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn non_empty_cron_expression(job: &PluginRegisteredCronJob) -> Option<&str> {
    job.cron_expression
        .as_ref()
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
