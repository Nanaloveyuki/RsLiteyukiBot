use super::*;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
// 必要测试
fn read_workspace_file_returns_empty_range_when_start_line_exceeds_file() {
    let workspace_root = temp_path("workspace-read-empty-range");
    let file_path = workspace_root.join("notes.txt");
    fs::create_dir_all(&workspace_root).expect("workspace root should exist");
    fs::write(&file_path, "first\nsecond\n").expect("file should be written");

    let output = read_workspace_file(
        workspace_root.as_path(),
        &json!({"path": "notes.txt", "start_line": 9}),
    )
    .expect("read should succeed");

    assert!(matches!(
        output,
        LlmToolOutput::Text(ref text) if text.contains("[requested line range is empty]")
    ));

    let _ = fs::remove_file(file_path);
    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
// 必要测试
fn read_workspace_file_rejects_missing_leaf_under_external_symlink() {
    let workspace_root = temp_path("workspace-root");
    let external_root = temp_path("workspace-external");
    fs::create_dir_all(&workspace_root).expect("workspace root should exist");
    fs::create_dir_all(&external_root).expect("external root should exist");

    let symlink_path = workspace_root.join("linked-outside");
    create_dir_symlink_for_test(external_root.as_path(), symlink_path.as_path())
        .expect("directory link creation should succeed for the escape regression test");

    let error = read_workspace_file(
        workspace_root.as_path(),
        &json!({ "path": "linked-outside/missing.txt" }),
    )
    .expect_err("missing leaf under external symlink should be rejected");
    assert!(
        error.to_string().contains("escapes the workspace root"),
        "unexpected error: {error}"
    );

    let _ = remove_dir_symlink_for_test(&symlink_path);
    let _ = fs::remove_dir_all(&workspace_root);
    let _ = fs::remove_dir_all(&external_root);
}

fn temp_path(label: &str) -> PathBuf {
    let process_id = std::process::id();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "liteyuki-workspace-access-test-{label}-{process_id}-{unique}"
    ))
}

#[cfg(unix)]
fn create_dir_symlink_for_test(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(unix)]
fn remove_dir_symlink_for_test(link: &Path) -> std::io::Result<()> {
    fs::remove_file(link)
}

#[cfg(windows)]
fn create_dir_symlink_for_test(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link).or_else(|err| {
        if err.kind() != ErrorKind::PermissionDenied {
            return Err(err);
        }
        let status = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                link.display().to_string().as_str(),
                target.display().to_string().as_str(),
            ])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "mklink /J failed with status {status}"
            )))
        }
    })
}

#[cfg(windows)]
fn remove_dir_symlink_for_test(link: &Path) -> std::io::Result<()> {
    fs::remove_dir(link)
}
