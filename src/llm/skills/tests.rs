use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("liteyuki-skill-test-{label}-{unique}"))
}

#[test]
// 必要测试
fn extract_skill_description_reads_frontmatter() {
    let content = "---\ndescription: Example skill\n---\n# Skill";
    assert_eq!(
        extract_skill_description(content).as_deref(),
        Some("Example skill")
    );
}

#[test]
// 必要测试
fn extract_skill_description_handles_crlf_frontmatter() {
    let content = "---\r\ndescription: Windows skill\r\n---\r\n# Skill";
    assert_eq!(
        extract_skill_description(content).as_deref(),
        Some("Windows skill")
    );
}

#[test]
// 必要测试
fn list_skills_scans_repo_local_layout() {
    let root = temp_path("scan");
    let skill_root = root.join("skills").join("demo");
    fs::create_dir_all(&skill_root).expect("skill dir should be created");
    fs::write(
        skill_root.join("SKILL.md"),
        "---\ndescription: Demo skill\n---\n# Demo",
    )
    .expect("skill file should be written");

    let manager = SkillManager::for_workspace(root.as_path());
    let skills = manager.list_skills().expect("skills should scan");
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "demo");
    assert_eq!(skills[0].description.as_deref(), Some("Demo skill"));

    let _ = fs::remove_file(skill_root.join("SKILL.md"));
    let _ = fs::remove_dir_all(root);
}

#[test]
// 必要测试
fn list_skills_prefers_managed_root_over_workspace_root() {
    let root = temp_path("skill-roots");
    let managed_root = root.join(".liteyuki").join("skills");
    let legacy_root = root.join("skills");
    fs::create_dir_all(managed_root.join("demo")).expect("managed skill dir should exist");
    fs::create_dir_all(legacy_root.join("demo")).expect("legacy skill dir should exist");
    fs::write(
        managed_root.join("demo").join("SKILL.md"),
        "---\ndescription: Managed skill\n---\n# Managed",
    )
    .expect("managed skill should be written");
    fs::write(
        legacy_root.join("demo").join("SKILL.md"),
        "---\ndescription: Legacy skill\n---\n# Legacy",
    )
    .expect("legacy skill should be written");

    let manager = SkillManager::from_roots(
        root.clone(),
        managed_root.clone(),
        vec![managed_root, legacy_root],
    );
    let skills = manager.list_skills().expect("skills should scan");
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].description.as_deref(), Some("Managed skill"));

    let _ = fs::remove_dir_all(root);
}
