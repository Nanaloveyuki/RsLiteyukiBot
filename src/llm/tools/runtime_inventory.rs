use std::collections::HashSet;

use crate::llm::client::LlmFunctionTool;

use super::tool_state::ToolStateStore;
use super::tool_types::ManagedTool;
use super::{TOOL_CATEGORY_EXTERNAL, TOOL_CATEGORY_MCP, ToolDescriptor, ToolOrigin};
use crate::llm::mcp::McpBoundTool;

pub(super) struct RuntimeToolInventory {
    pub(super) tools: Vec<ManagedTool>,
    pub(super) warnings: Vec<String>,
}

pub(super) fn to_managed_mcp_tool(binding: McpBoundTool) -> ManagedTool {
    let description = if binding.description.trim().is_empty() {
        format!(
            "Remote MCP tool '{}' exposed by server '{}'.",
            binding.remote_name, binding.server_name
        )
    } else {
        format!(
            "{} (remote MCP tool '{}', server '{}')",
            binding.description, binding.remote_name, binding.server_name
        )
    };
    ManagedTool {
        descriptor: ToolDescriptor {
            name: binding.name,
            description,
            parameters: binding.parameters,
            category: TOOL_CATEGORY_MCP.to_string(),
            when_to_use: format!(
                "Use when the MCP server '{}' exposes the capability you need.",
                binding.server_name
            ),
            origin: ToolOrigin::Mcp {
                server: binding.server_name,
            },
            strict: true,
        },
        tool: binding.tool,
    }
}

pub(super) fn collect_execution_descriptors(
    tools: &[ManagedTool],
    tool_state: &ToolStateStore,
    extra_tools: &[LlmFunctionTool],
) -> Vec<ToolDescriptor> {
    tools
        .iter()
        .filter(|tool| tool_state.is_active(tool.descriptor.name.as_str()))
        .map(|tool| tool.descriptor.clone())
        .chain(extra_tools.iter().map(external_tool_descriptor))
        .collect()
}

pub(super) fn collect_external_descriptors(extra_tools: &[LlmFunctionTool]) -> Vec<ToolDescriptor> {
    extra_tools.iter().map(external_tool_descriptor).collect()
}

pub(super) fn retain_active_tools(tools: &mut Vec<ManagedTool>, tool_state: &ToolStateStore) {
    tools.retain(|tool| tool_state.is_active(tool.descriptor.name.as_str()));
}

pub(super) fn merge_runtime_tools(
    tools: &[ManagedTool],
    extra_tools: &[LlmFunctionTool],
) -> Result<Vec<LlmFunctionTool>, String> {
    let mut merged_tools = Vec::new();
    let mut seen_names = HashSet::new();
    for tool in tools.iter().map(|tool| tool.tool.clone()) {
        if !seen_names.insert(tool.name.clone()) {
            return Err(format!("duplicate runtime tool name '{}'", tool.name));
        }
        merged_tools.push(tool);
    }
    for tool in extra_tools {
        if !seen_names.insert(tool.name.clone()) {
            return Err(format!(
                "duplicate tool name '{}' between runtime tools and extra tools",
                tool.name
            ));
        }
        merged_tools.push(tool.clone());
    }
    Ok(merged_tools)
}

fn external_tool_descriptor(tool: &LlmFunctionTool) -> ToolDescriptor {
    ToolDescriptor {
        name: tool.name.clone(),
        description: tool
            .description
            .clone()
            .unwrap_or_else(|| format!("External runtime tool '{}'.", tool.name)),
        parameters: tool.parameters.clone(),
        category: TOOL_CATEGORY_EXTERNAL.to_string(),
        when_to_use: "Use when the current runtime injects an external tool for this task."
            .to_string(),
        origin: ToolOrigin::External,
        strict: tool.strict,
    }
}

#[cfg(test)]
#[path = "runtime_inventory/tests.rs"]
mod tests;
