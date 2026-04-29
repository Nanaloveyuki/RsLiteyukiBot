use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

#[test]
// 必要测试
fn read_skill_document_reads_and_truncates_skill_file() {
    let workspace_root = temp_path("skills-read");
    let skill_dir = workspace_root.join("skills").join("demo");
    fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    let large_body = format!("{}\n", "x".repeat(600));
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\ndescription: Demo skill\n---\n# Demo\n{large_body}"),
    )
    .expect("skill file should be written");
    let skill_manager = SkillManager::for_workspace(workspace_root.as_path());

    let truncated = read_skill_document(
        skill_manager.clone(),
        &json!({
            "skill_name": "demo",
            "max_chars": 256
        }),
    )
    .expect("skill should be readable");
    let full = read_skill_document(
        skill_manager,
        &json!({
            "skill_name": "demo"
        }),
    )
    .expect("skill should be readable without explicit limit");

    assert!(matches!(
        truncated,
        LlmToolOutput::Text(ref text) if text.contains("[truncated]")
    ));
    assert!(matches!(
        (&truncated, &full),
        (LlmToolOutput::Text(truncated_text), LlmToolOutput::Text(full_text))
            if truncated_text.len() < full_text.len()
    ));

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
// 必要测试
fn read_skill_document_reports_missing_skill() {
    let workspace_root = temp_path("skills-missing");
    fs::create_dir_all(&workspace_root).expect("workspace root should be created");
    let skill_manager = SkillManager::for_workspace(workspace_root.as_path());

    let error = read_skill_document(
        skill_manager,
        &json!({
            "skill_name": "missing"
        }),
    )
    .expect_err("missing skill should error");

    assert!(error.to_string().contains("was not found"));

    let _ = fs::remove_dir_all(workspace_root);
}

fn temp_path(label: &str) -> std::path::PathBuf {
    static NEXT_SUFFIX: AtomicU64 = AtomicU64::new(0);
    let process_id = std::process::id();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    let sequence = NEXT_SUFFIX.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "liteyuki-local-tools-test-{label}-{process_id}-{timestamp}-{sequence}"
    ))
}
