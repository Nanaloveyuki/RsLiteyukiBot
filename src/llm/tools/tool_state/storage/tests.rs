use super::{read_tool_state_source, tool_state_backup_path};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
// 必要测试
fn missing_primary_with_invalid_backup_surfaces_error() {
    let primary_path = temp_path("tool-state-primary.json");
    let backup_path = tool_state_backup_path(primary_path.as_path());
    fs::write(&backup_path, "{invalid json").expect("invalid backup should be written");

    let error = read_tool_state_source(primary_path.as_path(), backup_path.as_path())
        .expect_err("invalid backup should surface an error");

    assert!(error.contains("could not be restored"));

    let _ = fs::remove_file(backup_path);
}

fn temp_path(label: &str) -> PathBuf {
    let process_id = std::process::id();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "liteyuki-tool-state-test-{label}-{process_id}-{unique}"
    ))
}
