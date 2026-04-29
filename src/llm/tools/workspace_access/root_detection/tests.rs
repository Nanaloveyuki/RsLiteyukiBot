use super::detect_workspace_root;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
// 必要测试
fn detect_workspace_root_prefers_outermost_git_root() {
    let root = temp_path("git-root");
    let nested = root.join("crates").join("app").join("src");
    fs::create_dir_all(&nested).expect("nested dir should be created");
    fs::create_dir_all(root.join(".git")).expect("git dir should be created");
    fs::write(
        root.join("crates").join("app").join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("crate manifest should be written");

    assert_eq!(detect_workspace_root(nested.as_path()), root);

    let _ = fs::remove_dir_all(root);
}

#[test]
// 必要测试
fn detect_workspace_root_stops_at_nearest_git_boundary() {
    let root = temp_path("nested-git-root");
    let nested_repo = root.join("nested-repo");
    let start = nested_repo.join("src");
    fs::create_dir_all(&start).expect("nested dir should be created");
    fs::create_dir_all(root.join(".git")).expect("outer git dir should be created");
    fs::create_dir_all(nested_repo.join(".git")).expect("inner git dir should be created");

    assert_eq!(detect_workspace_root(start.as_path()), nested_repo);

    let _ = fs::remove_dir_all(root);
}

#[test]
// 必要测试
fn detect_workspace_root_prefers_workspace_manifest_inside_git_repo() {
    let root = temp_path("workspace-inside-git-root");
    let workspace_root = root.join("rust-workspace");
    let nested = workspace_root.join("crates").join("app").join("src");
    fs::create_dir_all(&nested).expect("nested dir should be created");
    fs::create_dir_all(root.join(".git")).expect("outer git dir should be created");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\"]\n",
    )
    .expect("workspace manifest should be written");
    fs::write(
        workspace_root.join("crates").join("app").join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("crate manifest should be written");

    assert_eq!(detect_workspace_root(nested.as_path()), workspace_root);

    let _ = fs::remove_dir_all(root);
}

#[test]
// 必要测试
fn detect_workspace_root_does_not_cross_nested_git_boundary_for_outer_workspace() {
    let root = temp_path("workspace-outside-nested-git-root");
    let nested_repo = root.join("nested-repo");
    let nested_start = nested_repo.join("src");
    fs::create_dir_all(&nested_start).expect("nested dir should be created");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"nested-repo\"]\n",
    )
    .expect("workspace manifest should be written");
    fs::create_dir_all(nested_repo.join(".git")).expect("inner git dir should be created");
    fs::write(
        nested_repo.join("Cargo.toml"),
        "[package]\nname = \"nested-repo\"\nversion = \"0.1.0\"\n",
    )
    .expect("crate manifest should be written");

    assert_eq!(detect_workspace_root(nested_start.as_path()), nested_repo);

    let _ = fs::remove_dir_all(root);
}

#[test]
// 必要测试
fn detect_workspace_root_prefers_workspace_manifest_without_git() {
    let root = temp_path("workspace-manifest-root");
    let nested = root.join("crates").join("app").join("src");
    fs::create_dir_all(&nested).expect("nested dir should be created");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\"]\n",
    )
    .expect("workspace manifest should be written");
    fs::write(
        root.join("crates").join("app").join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("crate manifest should be written");

    assert_eq!(detect_workspace_root(nested.as_path()), root);

    let _ = fs::remove_dir_all(root);
}

#[test]
// 必要测试
fn detect_workspace_root_falls_back_to_outermost_manifest() {
    let root = temp_path("outermost-manifest-root");
    let nested = root.join("subcrate").join("src");
    fs::create_dir_all(&nested).expect("nested dir should be created");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"root\"\nversion = \"0.1.0\"\n",
    )
    .expect("root manifest should be written");
    fs::write(
        root.join("subcrate").join("Cargo.toml"),
        "[package]\nname = \"subcrate\"\nversion = \"0.1.0\"\n",
    )
    .expect("nested manifest should be written");

    assert_eq!(detect_workspace_root(nested.as_path()), root);

    let _ = fs::remove_dir_all(root);
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
