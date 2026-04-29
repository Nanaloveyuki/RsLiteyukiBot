use super::*;
use std::path::Path;

use serde_json::json;

use crate::llm::tools::{TOOL_CATEGORY_WORKSPACE, ToolOrigin};

#[test]
// 必要测试
fn merge_system_prompt_sections_skips_empty_values() {
    let merged = merge_system_prompt_sections(Some("base"), [None, Some("extra")]);
    assert_eq!(merged.as_deref(), Some("base\n\nextra"));
}

#[test]
// 必要测试
fn build_runtime_inventory_prompt_includes_typed_category_summary() {
    let descriptors = vec![ToolDescriptor {
        name: "workspace_read_file".to_string(),
        description: "Read".to_string(),
        parameters: json!({}),
        category: TOOL_CATEGORY_WORKSPACE.to_string(),
        when_to_use: "Inspect a file".to_string(),
        origin: ToolOrigin::Local,
        strict: true,
    }];
    let skill_manager = SkillManager::for_workspace(Path::new("."));

    let prompt = build_runtime_inventory_prompt(descriptors.as_slice(), &[], &[], &skill_manager)
        .expect("prompt should be built");

    assert!(prompt.contains("Available executable tool categories:"));
    assert!(
        prompt.contains("- workspace (1 tools): Inspect workspace files and repository structure.")
    );
}
