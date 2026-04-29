use serde::Serialize;
use serde_json::Value;

use crate::llm::client::{LlmClientError, LlmFunctionTool, LlmToolOutput};
use crate::llm::skills::SkillCatalogEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolOrigin {
    Local,
    External,
    Mcp { server: String },
}

#[derive(Debug, Clone)]
pub(crate) struct ToolDescriptor {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
    pub(crate) category: String,
    pub(crate) when_to_use: String,
    pub(crate) origin: ToolOrigin,
    pub(crate) strict: bool,
}

// 外部调用
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolCatalogEntry {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
    pub(crate) category: String,
    pub(crate) when_to_use: String,
    pub(crate) origin: String,
    pub(crate) strict: bool,
    pub(crate) active: bool,
}

// 外部调用
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolCatalogSnapshot {
    pub(crate) tools: Vec<ToolCatalogEntry>,
    pub(crate) warnings: Vec<String>,
}

// 外部调用
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillCatalogSnapshot {
    pub(crate) skills: Vec<SkillCatalogEntry>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CapabilityBundle {
    pub(crate) tools: Vec<LlmFunctionTool>,
    pub(crate) system_prompt: Option<String>,
}

#[derive(Clone)]
pub(super) struct ManagedTool {
    pub(super) descriptor: ToolDescriptor,
    pub(super) tool: LlmFunctionTool,
}

pub(super) fn new_managed_tool<F, Fut>(descriptor: ToolDescriptor, handler: F) -> ManagedTool
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<LlmToolOutput, LlmClientError>> + Send + 'static,
{
    let tool = LlmFunctionTool::new(
        descriptor.name.clone(),
        descriptor.parameters.clone(),
        handler,
    )
    .with_description(descriptor.description.clone())
    .with_strict(descriptor.strict);

    ManagedTool { descriptor, tool }
}
