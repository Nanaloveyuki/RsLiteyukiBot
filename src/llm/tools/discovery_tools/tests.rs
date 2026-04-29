use super::*;
use crate::llm::tools::TOOL_CATEGORY_EXTERNAL;

#[test]
// 必要测试
fn list_tools_in_category_returns_matching_entries() {
    let descriptors = vec![ToolDescriptor {
        name: "workspace_read_file".to_string(),
        description: "Read".to_string(),
        parameters: json!({}),
        category: TOOL_CATEGORY_WORKSPACE.to_string(),
        when_to_use: "Inspect a file".to_string(),
        origin: ToolOrigin::Local,
        strict: true,
    }];

    let output = list_tools_in_category(&descriptors, &json!({"category": "workspace"}))
        .expect("category should exist");
    assert!(matches!(output, LlmToolOutput::Json(_)));
}

#[test]
// 必要测试
fn get_tool_schema_reports_external_origin() {
    let descriptors = vec![ToolDescriptor {
        name: "external_demo".to_string(),
        description: "External".to_string(),
        parameters: json!({}),
        category: TOOL_CATEGORY_EXTERNAL.to_string(),
        when_to_use: "Use external runtime tool".to_string(),
        origin: ToolOrigin::External,
        strict: true,
    }];

    let output = get_tool_schema(&descriptors, &json!({"tool_name": "external_demo"}))
        .expect("tool schema should exist");

    assert!(matches!(
        output,
        LlmToolOutput::Json(ref value) if value["origin"] == "external"
    ));
}

#[test]
// 必要测试
fn summarize_categories_includes_discovery_helpers() {
    let summaries = summarize_categories(discovery_helper_descriptors().as_slice());
    assert!(summaries.iter().any(|summary| {
        summary.name == TOOL_CATEGORY_DISCOVERY && summary.estimated_tool_count == 3
    }));
}
