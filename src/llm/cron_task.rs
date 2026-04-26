#![allow(dead_code)]

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, FixedOffset, Offset, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::config_paths::resolve_preferred_plugin_cron_state_path;
use liteyukibot_core::{PluginCapabilitySnapshot, PluginRegisteredCronJob};

const SCHEDULER_LOOKAHEAD_MINUTES: i64 = 366 * 24 * 60;
const EXECUTABLE_CRON_JOB_TYPES: &[&str] = &["basic"];

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CronTaskStateDocument {
    #[serde(default)]
    jobs: Vec<CronTaskStateEntry>,
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
struct CronTaskRuntimeEntry {
    schedule_signature: Option<String>,
    next_run_time: Option<String>,
    last_run_time: Option<String>,
    last_error: Option<String>,
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
                let entries = document
                    .jobs
                    .into_iter()
                    .map(|entry| {
                        (
                            CronTaskKey::new(entry.plugin_id.clone(), entry.job_id.clone()),
                            entry.into(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
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
        if let Some(parent) = self.path.parent()
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
            jobs: self
                .entries
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
        let temp_path = cron_state_temp_path(self.path.as_path());
        let backup_path = cron_state_backup_path(self.path.as_path());
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
        if self.path.exists() {
            fs::rename(&self.path, &backup_path).map_err(|err| {
                let _ = fs::remove_file(&temp_path);
                format!(
                    "failed to stage previous plugin cron state file '{}': {err}",
                    self.path.display()
                )
            })?;
        }
        if let Err(err) = fs::rename(&temp_path, &self.path) {
            let _ = fs::remove_file(&temp_path);
            if backup_path.exists() {
                let _ = fs::rename(&backup_path, &self.path);
            }
            return Err(format!(
                "failed to replace plugin cron state file '{}': {err}",
                self.path.display()
            ));
        }
        if backup_path.exists() {
            let _ = fs::remove_file(backup_path);
        }
        Ok(())
    }
}

pub(crate) fn cron_job_is_host_executable(job: &PluginRegisteredCronJob) -> bool {
    job.enabled
        && EXECUTABLE_CRON_JOB_TYPES
            .iter()
            .any(|kind| kind == &job.job_type.as_str())
        && (job.cron_expression.is_some() || job.run_once)
}

fn apply_scheduler_overlay(
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

fn compute_next_run_time(
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
            return next_cron_occurrence(expression, job.timezone.as_deref(), schedule_reference)
                .map(|value| Some(value.to_rfc3339()));
        }
        return Ok(Some(now.to_rfc3339()));
    }

    let Some(expression) = non_empty_cron_expression(job) else {
        return Ok(None);
    };
    next_cron_occurrence(expression, job.timezone.as_deref(), schedule_reference)
        .map(|value| Some(value.to_rfc3339()))
}

fn non_empty_cron_expression(job: &PluginRegisteredCronJob) -> Option<&str> {
    job.cron_expression
        .as_ref()
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn job_schedule_signature(job: &PluginRegisteredCronJob) -> String {
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

fn next_cron_occurrence(
    expression: &str,
    timezone: Option<&str>,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    let fields = parse_cron_expression(expression)?;
    let offset = parse_timezone_offset(timezone)?;
    let mut candidate = after.with_timezone(&offset);
    candidate = candidate
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .ok_or_else(|| "failed to normalize cron candidate".to_string())?
        + Duration::minutes(1);

    for _ in 0..SCHEDULER_LOOKAHEAD_MINUTES {
        if cron_fields_match(&fields, candidate) {
            return Ok(candidate.with_timezone(&Utc));
        }
        candidate += Duration::minutes(1);
    }

    Err(format!(
        "cron expression '{}' has no matching run time within the scheduler lookahead window",
        expression.trim()
    ))
}

#[derive(Debug, Clone)]
struct ParsedCronFields {
    minute: CronField,
    hour: CronField,
    day_of_month: CronField,
    month: CronField,
    day_of_week: CronField,
}

#[derive(Debug, Clone)]
struct CronField {
    any: bool,
    values: HashSet<u32>,
}

fn parse_cron_expression(expression: &str) -> Result<ParsedCronFields, String> {
    let parts = expression
        .split_whitespace()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() != 5 {
        return Err("cron expression must contain exactly five fields".to_string());
    }

    Ok(ParsedCronFields {
        minute: parse_cron_field(parts[0], 0, 59)?,
        hour: parse_cron_field(parts[1], 0, 23)?,
        day_of_month: parse_cron_field(parts[2], 1, 31)?,
        month: parse_cron_field(parts[3], 1, 12)?,
        day_of_week: parse_cron_field(parts[4], 0, 7)?,
    })
}

fn parse_cron_field(source: &str, min: u32, max: u32) -> Result<CronField, String> {
    let trimmed = source.trim();
    if trimmed == "*" {
        return Ok(CronField {
            any: true,
            values: HashSet::new(),
        });
    }

    let mut values = HashSet::new();
    for segment in trimmed.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            return Err(format!("invalid cron field '{}'", source));
        }

        if let Some(step_source) = segment.strip_prefix("*/") {
            let step = parse_cron_number(step_source, min, max)?;
            if step == 0 {
                return Err(format!("invalid cron step '{}'", segment));
            }
            let mut value = min;
            while value <= max {
                values.insert(value);
                value = value.saturating_add(step);
                if value == 0 {
                    break;
                }
            }
            continue;
        }

        if let Some((range_source, step_source)) = segment.split_once('/') {
            let step = parse_cron_number(step_source, min, max)?;
            if step == 0 {
                return Err(format!("invalid cron step '{}'", segment));
            }
            let (start, end) = parse_cron_range(range_source, min, max)?;
            let mut value = start;
            while value <= end {
                values.insert(value);
                value = value.saturating_add(step);
                if value == 0 {
                    break;
                }
            }
            continue;
        }

        if segment.contains('-') {
            let (start, end) = parse_cron_range(segment, min, max)?;
            for value in start..=end {
                values.insert(value);
            }
            continue;
        }

        values.insert(parse_cron_number(segment, min, max)?);
    }

    Ok(CronField { any: false, values })
}

fn parse_cron_range(segment: &str, min: u32, max: u32) -> Result<(u32, u32), String> {
    let (start, end) = segment
        .split_once('-')
        .ok_or_else(|| format!("invalid cron range '{}'", segment))?;
    let start = parse_cron_number(start, min, max)?;
    let end = parse_cron_number(end, min, max)?;
    if start > end {
        return Err(format!("invalid descending cron range '{}'", segment));
    }
    Ok((start, end))
}

fn parse_cron_number(source: &str, min: u32, max: u32) -> Result<u32, String> {
    let value = source
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("invalid cron number '{}'", source.trim()))?;
    if value < min || value > max {
        return Err(format!(
            "cron value '{}' is outside the supported range {}..={}",
            value, min, max
        ));
    }
    Ok(value)
}

fn cron_fields_match(fields: &ParsedCronFields, candidate: DateTime<FixedOffset>) -> bool {
    let minute_matches = field_matches(&fields.minute, candidate.minute());
    let hour_matches = field_matches(&fields.hour, candidate.hour());
    let month_matches = field_matches(&fields.month, candidate.month());
    let day_of_month_matches = field_matches(&fields.day_of_month, candidate.day());
    let weekday = candidate.weekday().num_days_from_sunday();
    let day_of_week_matches = field_matches(&fields.day_of_week, weekday)
        || (weekday == 0 && field_matches(&fields.day_of_week, 7));

    minute_matches
        && hour_matches
        && month_matches
        && day_match(
            day_of_month_matches,
            day_of_week_matches,
            &fields.day_of_month,
            &fields.day_of_week,
        )
}

fn day_match(
    day_of_month_matches: bool,
    day_of_week_matches: bool,
    day_of_month: &CronField,
    day_of_week: &CronField,
) -> bool {
    match (day_of_month.any, day_of_week.any) {
        (true, true) => true,
        (false, true) => day_of_month_matches,
        (true, false) => day_of_week_matches,
        (false, false) => day_of_month_matches || day_of_week_matches,
    }
}

fn field_matches(field: &CronField, value: u32) -> bool {
    field.any || field.values.contains(&value)
}

fn parse_timezone_offset(source: Option<&str>) -> Result<FixedOffset, String> {
    let Some(source) = source.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Utc.fix());
    };
    if source.eq_ignore_ascii_case("utc") || source.eq_ignore_ascii_case("z") {
        return Ok(Utc.fix());
    }

    let source = source.replace("UTC", "").replace("utc", "");
    let source = source.trim().to_string();
    if source.is_empty() {
        return Ok(Utc.fix());
    }

    let sign = if source.starts_with('-') { -1 } else { 1 };
    let numeric = source.trim_start_matches(['+', '-']);
    let (hours, minutes) = if let Some((hours, minutes)) = numeric.split_once(':') {
        (
            hours
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
            minutes
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
        )
    } else if numeric.len() == 4 {
        (
            numeric[..2]
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
            numeric[2..]
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
        )
    } else {
        (
            numeric
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
            0,
        )
    };
    let seconds = sign * (hours * 3600 + minutes * 60);
    FixedOffset::east_opt(seconds)
        .ok_or_else(|| format!("invalid cron timezone '{}'", source.trim()))
}

fn parse_timestamp(source: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(source)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn read_state_document(
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

fn cron_state_backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("liteyuki-cron-task-{label}-{unique}.json"))
    }

    fn sample_job() -> PluginRegisteredCronJob {
        PluginRegisteredCronJob {
            plugin_id: "demo".to_string(),
            job_id: "demo:basic:1".to_string(),
            job_type: "basic".to_string(),
            name: "demo".to_string(),
            description: "demo".to_string(),
            cron_expression: Some("*/5 * * * *".to_string()),
            enabled: true,
            persistent: true,
            ..PluginRegisteredCronJob::default()
        }
    }

    #[test]
    fn cron_expression_supports_interval_minutes() {
        let now = DateTime::parse_from_rfc3339("2026-04-26T10:02:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let next = next_cron_occurrence("*/5 * * * *", None, now).expect("next run");
        assert_eq!(next.to_rfc3339(), "2026-04-26T10:05:00+00:00");
    }

    #[test]
    fn scheduler_recovers_from_backup_state_file() {
        let path = temp_path("backup");
        let backup = cron_state_backup_path(path.as_path());
        fs::write(&path, "{invalid json").expect("invalid primary should be written");
        fs::write(
            &backup,
            "{\n  \"jobs\": [\n    {\n      \"pluginId\": \"demo\",\n      \"jobId\": \"demo:basic:1\",\n      \"nextRunTime\": \"2026-04-26T10:05:00Z\"\n    }\n  ]\n}\n",
        )
        .expect("backup should be written");

        let scheduler = PluginCronTaskScheduler::from_config_path(path.as_path());
        assert!(
            scheduler
                .warnings()
                .iter()
                .any(|warning| warning.contains("restored state from backup"))
        );

        let _ = fs::remove_file(path);
        let _ = fs::remove_file(backup);
    }

    #[test]
    fn sync_snapshot_overlays_next_run_time() {
        let path = temp_path("sync");
        let mut scheduler = PluginCronTaskScheduler::from_config_path(path.as_path());
        let mut snapshot = PluginCapabilitySnapshot {
            plugin_id: "demo".to_string(),
            cron_jobs: vec![sample_job()],
            ..PluginCapabilitySnapshot::default()
        };
        let now = DateTime::parse_from_rfc3339("2026-04-26T10:02:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);

        scheduler
            .sync_snapshot(&mut snapshot, now)
            .expect("snapshot should sync");
        assert_eq!(
            snapshot.cron_jobs[0].next_run_time.as_deref(),
            Some("2026-04-26T10:05:00+00:00")
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn mark_job_success_disables_run_once_job() {
        let path = temp_path("run-once");
        let mut scheduler = PluginCronTaskScheduler::from_config_path(path.as_path());
        let mut job = sample_job();
        job.run_once = true;
        job.cron_expression = None;
        let mut snapshot = PluginCapabilitySnapshot {
            plugin_id: "demo".to_string(),
            cron_jobs: vec![job.clone()],
            ..PluginCapabilitySnapshot::default()
        };
        let now = DateTime::parse_from_rfc3339("2026-04-26T10:02:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);

        scheduler
            .sync_snapshot(&mut snapshot, now)
            .expect("snapshot should sync");
        assert_eq!(
            snapshot.cron_jobs[0].next_run_time.as_deref(),
            Some("2026-04-26T10:02:00+00:00")
        );

        let key = CronTaskKey::new("demo", "demo:basic:1");
        scheduler
            .mark_job_success(&key, &job, now)
            .expect("run-once job should mark success");

        let mut refreshed = PluginCapabilitySnapshot {
            plugin_id: "demo".to_string(),
            cron_jobs: vec![job],
            ..PluginCapabilitySnapshot::default()
        };
        scheduler
            .sync_snapshot(&mut refreshed, now)
            .expect("snapshot should refresh");
        assert_eq!(refreshed.cron_jobs[0].next_run_time, None);
        assert_eq!(
            refreshed.cron_jobs[0].last_run_time.as_deref(),
            Some("2026-04-26T10:02:00+00:00")
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn mark_job_error_keeps_run_once_job_completed() {
        let path = temp_path("run-once-error");
        let mut scheduler = PluginCronTaskScheduler::from_config_path(path.as_path());
        let mut job = sample_job();
        job.run_once = true;
        job.cron_expression = None;
        let now = DateTime::parse_from_rfc3339("2026-04-26T10:02:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let key = CronTaskKey::new("demo", "demo:basic:1");

        scheduler
            .mark_job_error(&key, &job, now, "boom")
            .expect("run-once error should be recorded");

        let mut refreshed = PluginCapabilitySnapshot {
            plugin_id: "demo".to_string(),
            cron_jobs: vec![job],
            ..PluginCapabilitySnapshot::default()
        };
        scheduler
            .sync_snapshot(&mut refreshed, now)
            .expect("snapshot should refresh");
        assert_eq!(refreshed.cron_jobs[0].next_run_time, None);
        assert_eq!(
            refreshed.cron_jobs[0].last_run_time.as_deref(),
            Some("2026-04-26T10:02:00+00:00")
        );
        assert_eq!(refreshed.cron_jobs[0].last_error.as_deref(), Some("boom"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_primary_state_file_does_not_block_snapshot_sync() {
        let path = temp_path("invalid-primary");
        fs::write(&path, "{invalid json").expect("invalid state file should be written");
        let mut scheduler = PluginCronTaskScheduler::from_config_path(path.as_path());
        let mut snapshot = PluginCapabilitySnapshot {
            plugin_id: "demo".to_string(),
            cron_jobs: vec![sample_job()],
            ..PluginCapabilitySnapshot::default()
        };
        let now = DateTime::parse_from_rfc3339("2026-04-26T10:02:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);

        scheduler
            .sync_snapshot(&mut snapshot, now)
            .expect("snapshot sync should degrade instead of failing");
        assert_eq!(
            snapshot.cron_jobs[0].next_run_time.as_deref(),
            Some("2026-04-26T10:05:00+00:00")
        );
        assert!(
            scheduler
                .warnings()
                .iter()
                .any(|warning| warning.contains("invalid json"))
        );

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(cron_state_backup_path(path.as_path()));
    }

    #[test]
    fn cron_expression_respects_timezone_offsets() {
        let now = DateTime::parse_from_rfc3339("2026-04-26T10:02:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let next = next_cron_occurrence("0 9 * * *", Some("+08:00"), now).expect("next run");
        assert_eq!(next.to_rfc3339(), "2026-04-27T01:00:00+00:00");
    }
}
