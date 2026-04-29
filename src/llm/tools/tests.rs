use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::llm::client::{LlmFunctionTool, LlmToolOutput};

use super::tool_state::tool_state_backup_path;
use super::{NON_TOGGLEABLE_TOOL_NAMES, ToolManager};

fn isolated_manager_for_workspace(workspace_root: &Path) -> ToolManager {
    isolated_manager_with_tool_state(workspace_root, temp_path("tool-state.json").as_path())
}

fn isolated_manager_with_tool_state(workspace_root: &Path, tool_state_path: &Path) -> ToolManager {
    ToolManager::for_test_workspace(
        workspace_root,
        tool_state_path,
        temp_path("missing-mcp-config.json").as_path(),
    )
}

#[tokio::test]
async fn tool_manager_builds_local_runtime_bundle() {
    let root = temp_path("bundle");
    let skill_root = root.join("skills").join("demo");
    fs::create_dir_all(&skill_root).expect("skill dir should be created");
    fs::write(
        skill_root.join("SKILL.md"),
        "---\ndescription: Demo skill\n---\n# Demo",
    )
    .expect("skill file should be written");

    let manager = isolated_manager_for_workspace(root.as_path());
    let bundle = manager
        .build_runtime_bundle(&[])
        .await
        .expect("bundle should build");
    let tool_names = bundle
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();

    assert!(tool_names.contains(&"workspace_read_file"));
    assert!(tool_names.contains(&"read_skill_document"));
    assert!(tool_names.contains(&"list_tool_categories"));
    assert!(
        bundle
            .system_prompt
            .as_deref()
            .is_some_and(|prompt| prompt.contains("Available skills:"))
    );

    let _ = fs::remove_file(skill_root.join("SKILL.md"));
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn broken_skills_inventory_does_not_break_bundle_building() {
    let root = temp_path("broken-skills");
    fs::create_dir_all(&root).expect("root should exist");
    fs::write(root.join("skills"), "not a directory").expect("blocking file should exist");

    let manager = isolated_manager_for_workspace(root.as_path());
    let bundle = manager
        .build_runtime_bundle(&[])
        .await
        .expect("bundle should still build");

    assert!(
        bundle
            .tools
            .iter()
            .any(|tool| tool.name == "workspace_read_file")
    );
    assert!(
        bundle
            .system_prompt
            .as_deref()
            .is_some_and(|prompt| prompt.contains("skills inventory unavailable"))
    );

    let _ = fs::remove_file(root.join("skills"));
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn tool_manager_persists_disabled_tools_and_skips_them_in_runtime_bundle() {
    let root = temp_path("tool-toggle");
    fs::create_dir_all(&root).expect("root should exist");
    let tool_state_path = temp_path("tool-state.json");

    let mut manager = isolated_manager_with_tool_state(root.as_path(), tool_state_path.as_path());
    manager
        .set_tool_active("workspace_read_file", false)
        .expect("tool state should persist");

    let snapshot = manager.describe_runtime_tools(&[]).await;
    assert!(
        snapshot
            .tools
            .iter()
            .any(|tool| tool.name == "workspace_read_file" && !tool.active)
    );

    let bundle = manager
        .build_runtime_bundle(&[])
        .await
        .expect("bundle should build");
    assert!(
        bundle
            .tools
            .iter()
            .all(|tool| tool.name != "workspace_read_file")
    );

    let reloaded = isolated_manager_with_tool_state(root.as_path(), tool_state_path.as_path());
    let reloaded_snapshot = reloaded.describe_runtime_tools(&[]).await;
    assert!(
        reloaded_snapshot
            .tools
            .iter()
            .any(|tool| tool.name == "workspace_read_file" && !tool.active)
    );

    let persisted =
        fs::read_to_string(&tool_state_path).expect("tool state file should be written");
    assert!(persisted.contains("\"workspace_read_file\": false"));

    let _ = fs::remove_file(tool_state_path);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn tool_manager_recovers_from_backup_without_overwriting_invalid_primary_state() {
    let root = temp_path("tool-backup");
    fs::create_dir_all(&root).expect("root should exist");
    let tool_state_path = temp_path("tool-state.json");
    let backup_path = tool_state_backup_path(tool_state_path.as_path());
    fs::write(&tool_state_path, "{invalid json").expect("invalid primary should be written");
    fs::write(
        &backup_path,
        "{\n  \"tools\": {\n    \"workspace_read_file\": false\n  }\n}\n",
    )
    .expect("backup state should be written");

    let manager = isolated_manager_with_tool_state(root.as_path(), tool_state_path.as_path());
    let snapshot = manager.describe_runtime_tools(&[]).await;
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("restored state from backup"))
    );
    assert!(
        snapshot
            .tools
            .iter()
            .any(|tool| tool.name == "workspace_read_file" && !tool.active)
    );
    assert_eq!(
        fs::read_to_string(&tool_state_path).expect("invalid primary should remain untouched"),
        "{invalid json"
    );

    let _ = fs::remove_file(tool_state_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn discovery_helpers_stay_active_even_if_state_file_marks_them_disabled() {
    let root = temp_path("tool-discovery-helper");
    fs::create_dir_all(&root).expect("root should exist");
    let tool_state_path = temp_path("tool-state.json");
    let disabled_helpers = NON_TOGGLEABLE_TOOL_NAMES
        .iter()
        .map(|name| format!("    \"{name}\": false"))
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        &tool_state_path,
        format!("{{\n  \"tools\": {{\n{disabled_helpers}\n  }}\n}}\n"),
    )
    .expect("tool state should be written");

    let manager = isolated_manager_with_tool_state(root.as_path(), tool_state_path.as_path());
    let snapshot = manager.describe_runtime_tools(&[]).await;
    for name in NON_TOGGLEABLE_TOOL_NAMES {
        assert!(
            snapshot
                .tools
                .iter()
                .any(|tool| tool.name == *name && tool.active),
            "{name} should remain active"
        );
    }
    let bundle = manager
        .build_runtime_bundle(&[])
        .await
        .expect("bundle should build");
    for name in NON_TOGGLEABLE_TOOL_NAMES {
        assert!(
            bundle.tools.iter().any(|tool| tool.name == *name),
            "{name} should remain callable"
        );
    }

    let _ = fs::remove_file(tool_state_path);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn runtime_prompt_includes_external_runtime_tool_category() {
    let root = temp_path("tool-external-runtime");
    fs::create_dir_all(&root).expect("root should exist");
    let manager = isolated_manager_for_workspace(root.as_path());
    let external_tool = LlmFunctionTool::new("external_demo", json!({}), |_| async {
        Ok(LlmToolOutput::Text("ok".to_string()))
    })
    .with_description("External demo tool");

    let bundle = manager
        .build_runtime_bundle(&[external_tool])
        .await
        .expect("bundle should build");
    let prompt = bundle.system_prompt.expect("prompt should exist");

    assert!(prompt.contains("external_runtime"));
    assert!(prompt.contains("Use external tools injected by the current runtime."));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn describe_runtime_tools_includes_external_runtime_tools() {
    let root = temp_path("tool-external-catalog");
    fs::create_dir_all(&root).expect("root should exist");
    let manager = isolated_manager_for_workspace(root.as_path());
    let external_tool = LlmFunctionTool::new("external_demo", json!({}), |_| async {
        Ok(LlmToolOutput::Text("ok".to_string()))
    })
    .with_description("External demo tool");

    let snapshot = manager.describe_runtime_tools(&[external_tool]).await;

    assert!(
        snapshot.tools.iter().any(|tool| {
            tool.name == "external_demo" && tool.origin == "external" && tool.active
        })
    );

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn describe_runtime_tools_skips_duplicate_external_tool_names() {
    let root = temp_path("tool-external-duplicates");
    fs::create_dir_all(&root).expect("root should exist");
    let manager = isolated_manager_for_workspace(root.as_path());
    let duplicate_a = LlmFunctionTool::new("external_demo", json!({}), |_| async {
        Ok(LlmToolOutput::Text("ok".to_string()))
    })
    .with_description("External demo tool A");
    let duplicate_b = LlmFunctionTool::new("external_demo", json!({}), |_| async {
        Ok(LlmToolOutput::Text("ok".to_string()))
    })
    .with_description("External demo tool B");

    let snapshot = manager
        .describe_runtime_tools(&[duplicate_a, duplicate_b])
        .await;

    assert_eq!(
        snapshot
            .tools
            .iter()
            .filter(|tool| tool.name == "external_demo")
            .count(),
        1
    );
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| { warning.contains("duplicate tool name 'external_demo' skipped") })
    );

    let _ = fs::remove_dir_all(root);
}

fn temp_path(label: &str) -> PathBuf {
    static NEXT_SUFFIX: AtomicU64 = AtomicU64::new(0);
    let process_id = std::process::id();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    let sequence = NEXT_SUFFIX.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "liteyuki-tools-test-{label}-{process_id}-{timestamp}-{sequence}"
    ))
}
