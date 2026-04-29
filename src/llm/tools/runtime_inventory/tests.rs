use serde_json::json;

use super::*;
use crate::llm::client::LlmToolOutput;

fn managed_tool(name: &str) -> ManagedTool {
    ManagedTool {
        descriptor: ToolDescriptor {
            name: name.to_string(),
            description: format!("{name} description"),
            parameters: json!({}),
            category: TOOL_CATEGORY_EXTERNAL.to_string(),
            when_to_use: format!("Use {name}"),
            origin: ToolOrigin::Local,
            strict: true,
        },
        tool: LlmFunctionTool::new(name, json!({}), |_| async {
            Ok(LlmToolOutput::Text("ok".to_string()))
        }),
    }
}

#[test]
// 必要测试
fn merge_runtime_tools_rejects_duplicate_runtime_tool_names() {
    let tools = vec![managed_tool("duplicate"), managed_tool("duplicate")];

    let error = merge_runtime_tools(tools.as_slice(), &[])
        .expect_err("duplicate runtime names should be rejected");

    assert!(error.contains("duplicate runtime tool name 'duplicate'"));
}

#[test]
// 必要测试
fn merge_runtime_tools_rejects_duplicate_extra_tool_names() {
    let tools = vec![managed_tool("shared")];
    let extra_tools = vec![LlmFunctionTool::new("shared", json!({}), |_| async {
        Ok(LlmToolOutput::Text("ok".to_string()))
    })];

    let error = merge_runtime_tools(tools.as_slice(), extra_tools.as_slice())
        .expect_err("duplicate runtime/extra names should be rejected");

    assert!(error.contains("duplicate tool name 'shared'"));
}

#[test]
// 必要测试
fn collect_external_descriptors_marks_external_origin() {
    let external_tools = vec![
        LlmFunctionTool::new("external_demo", json!({}), |_| async {
            Ok(LlmToolOutput::Text("ok".to_string()))
        })
        .with_description("External demo tool"),
    ];

    let descriptors = collect_external_descriptors(external_tools.as_slice());

    assert!(matches!(
        descriptors.as_slice(),
        [ToolDescriptor {
            name,
            description,
            category,
            origin: ToolOrigin::External,
            ..
        }] if name == "external_demo"
            && description == "External demo tool"
            && category == TOOL_CATEGORY_EXTERNAL
    ));
}
