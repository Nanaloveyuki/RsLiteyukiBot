use super::*;
use std::fs;
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
// 必要测试
fn cron_expression_supports_interval_minutes() {
    let now = DateTime::parse_from_rfc3339("2026-04-26T10:02:00Z")
        .expect("timestamp")
        .with_timezone(&Utc);
    let next = next_cron_occurrence("*/5 * * * *", None, now).expect("next run");
    assert_eq!(next.to_rfc3339(), "2026-04-26T10:05:00+00:00");
}

#[test]
// 必要测试
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
// 必要测试
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
// 必要测试
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
// 必要测试
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
// 必要测试
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
// 必要测试
fn cron_expression_respects_timezone_offsets() {
    let now = DateTime::parse_from_rfc3339("2026-04-26T10:02:00Z")
        .expect("timestamp")
        .with_timezone(&Utc);
    let next = next_cron_occurrence("0 9 * * *", Some("+08:00"), now).expect("next run");
    assert_eq!(next.to_rfc3339(), "2026-04-27T01:00:00+00:00");
}
