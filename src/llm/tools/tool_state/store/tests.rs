use super::ToolStateStore;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
// 必要测试
fn set_active_rolls_back_failed_disable_write() {
    let blocked_parent = temp_path("tool-state-blocked-parent");
    fs::write(&blocked_parent, "occupied").expect("blocking file should be created");
    let tool_state_path = blocked_parent.join("tool-state.json");

    let mut store = ToolStateStore::from_config_path(&tool_state_path);
    let error = store
        .set_active("workspace_read_file", false)
        .expect_err("persist should fail when parent path is blocked by a file");

    assert!(error.contains("failed to create tool state directory"));
    assert!(store.is_active("workspace_read_file"));
    assert!(!tool_state_path.exists());

    let _ = fs::remove_file(blocked_parent);
}

#[test]
// 必要测试
fn set_active_rolls_back_failed_enable_write() {
    let parent_dir = temp_path("tool-state-parent-dir");
    fs::create_dir_all(&parent_dir).expect("parent dir should be created");
    let tool_state_path = parent_dir.join("tool-state.json");
    fs::write(
        &tool_state_path,
        "{\n  \"tools\": {\n    \"workspace_read_file\": false\n  }\n}\n",
    )
    .expect("disabled state should be written");

    let mut store = ToolStateStore::from_config_path(&tool_state_path);
    fs::remove_file(&tool_state_path).expect("tool state file should be removed");
    fs::remove_dir_all(&parent_dir).expect("parent dir should be removed");
    fs::write(&parent_dir, "occupied").expect("blocking file should be created");

    let error = store
        .set_active("workspace_read_file", true)
        .expect_err("persist should fail when parent path is blocked by a file");

    assert!(error.contains("failed to create tool state directory"));
    assert!(!store.is_active("workspace_read_file"));
    assert!(!tool_state_path.exists());

    let _ = fs::remove_file(parent_dir);
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
