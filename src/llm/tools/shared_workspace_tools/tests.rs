use super::*;

use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn workspace_read_only_tool_caller_lists_files() {
    let workspace_root = temp_path("shared-list");
    fs::create_dir_all(workspace_root.join("src")).expect("workspace root should exist");
    fs::write(
        workspace_root.join("src").join("lib.rs"),
        "pub fn demo() {}\n",
    )
    .expect("file should be written");

    let caller = WorkspaceReadOnlyToolCaller::new(workspace_root.as_path());
    let output = caller
        .call(
            WorkspaceReadOnlyToolName::ListFiles,
            &json!({"path": "src"}),
        )
        .expect("list should succeed");

    assert!(matches!(
        output,
        LlmToolOutput::Json(ref value)
            if value["files"].as_array().is_some_and(|files| {
                files.iter().any(|entry| entry == "src/lib.rs")
            })
    ));

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_read_only_tool_caller_reads_file() {
    let workspace_root = temp_path("shared-read");
    fs::create_dir_all(&workspace_root).expect("workspace root should exist");
    fs::write(workspace_root.join("notes.txt"), "first\nsecond\n").expect("file should be written");

    let caller = WorkspaceReadOnlyToolCaller::new(workspace_root.as_path());
    let output = caller
        .call(
            WorkspaceReadOnlyToolName::ReadFile,
            &json!({"path": "notes.txt", "start_line": 2}),
        )
        .expect("read should succeed");

    assert!(matches!(
        output,
        LlmToolOutput::Text(ref text) if text.contains("   2 | second")
    ));

    let _ = fs::remove_dir_all(workspace_root);
}

fn temp_path(label: &str) -> PathBuf {
    let process_id = std::process::id();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "liteyuki-shared-workspace-tools-test-{label}-{process_id}-{unique}"
    ))
}
