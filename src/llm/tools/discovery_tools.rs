use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};

use crate::llm::client::{LlmClientError, LlmToolOutput};

use super::tool_arguments::required_string;
use super::tool_types::{ManagedTool, ToolCatalogEntry, new_managed_tool};
use super::{
    GET_TOOL_SCHEMA_TOOL_NAME, LIST_TOOL_CATEGORIES_TOOL_NAME, LIST_TOOLS_IN_CATEGORY_TOOL_NAME,
    TOOL_CATEGORY_DISCOVERY, TOOL_CATEGORY_MCP, TOOL_CATEGORY_SKILLS, TOOL_CATEGORY_WORKSPACE,
    ToolDescriptor, ToolOrigin,
};

#[derive(Debug, Clone, Serialize)]
pub(super) struct ToolCategorySummary {
    pub(super) name: String,
    pub(super) description: &'static str,
    pub(super) examples: Vec<String>,
    pub(super) estimated_tool_count: usize,
}

pub(super) fn build_discovery_tools(descriptors: Vec<ToolDescriptor>) -> Vec<ManagedTool> {
    let helper_descriptors = discovery_helper_descriptors();
    let mut catalog_descriptors = descriptors.clone();
    catalog_descriptors.extend(helper_descriptors.iter().cloned());
    let category_entries = summarize_categories(catalog_descriptors.as_slice());

    let categories_for_list = category_entries.clone();
    let list_categories = new_managed_tool(helper_descriptors[0].clone(), move |_| {
        let categories = categories_for_list.clone();
        async move { Ok(LlmToolOutput::Json(json!(categories))) }
    });

    let descriptors_for_category = catalog_descriptors.clone();
    let list_in_category = new_managed_tool(helper_descriptors[1].clone(), move |arguments| {
        let descriptors = descriptors_for_category.clone();
        async move { list_tools_in_category(descriptors.as_slice(), &arguments) }
    });

    let schema_descriptors = catalog_descriptors;
    let get_schema = new_managed_tool(helper_descriptors[2].clone(), move |arguments| {
        let descriptors = schema_descriptors.clone();
        async move { get_tool_schema(descriptors.as_slice(), &arguments) }
    });

    vec![list_categories, list_in_category, get_schema]
}

fn discovery_helper_descriptors() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: LIST_TOOL_CATEGORIES_TOOL_NAME.to_string(),
            description: "List the available tool categories with short descriptions.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            category: TOOL_CATEGORY_DISCOVERY.to_string(),
            when_to_use:
                "Use when you need to discover which tool area is most relevant before picking a tool."
                    .to_string(),
            origin: ToolOrigin::Local,
            strict: true,
        },
        ToolDescriptor {
            name: LIST_TOOLS_IN_CATEGORY_TOOL_NAME.to_string(),
            description: "List tools in a specific category without dumping their full schema."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Tool category name returned by list_tool_categories."
                    }
                },
                "required": ["category"],
                "additionalProperties": false
            }),
            category: TOOL_CATEGORY_DISCOVERY.to_string(),
            when_to_use:
                "Use when you already know the rough tool area and want to choose a specific tool."
                    .to_string(),
            origin: ToolOrigin::Local,
            strict: true,
        },
        ToolDescriptor {
            name: GET_TOOL_SCHEMA_TOOL_NAME.to_string(),
            description: "Return the schema and usage guidance for a specific tool.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tool_name": {
                        "type": "string",
                        "description": "Exact tool name."
                    }
                },
                "required": ["tool_name"],
                "additionalProperties": false
            }),
            category: TOOL_CATEGORY_DISCOVERY.to_string(),
            when_to_use:
                "Use when you are unsure about a tool's parameters or constraints before calling it."
                    .to_string(),
            origin: ToolOrigin::Local,
            strict: true,
        },
    ]
}

#[allow(dead_code)]
pub(super) fn tool_descriptor_to_catalog_entry(
    descriptor: ToolDescriptor,
    active: bool,
) -> ToolCatalogEntry {
    ToolCatalogEntry {
        name: descriptor.name,
        description: descriptor.description,
        parameters: descriptor.parameters,
        category: descriptor.category,
        when_to_use: descriptor.when_to_use,
        origin: origin_label(&descriptor.origin),
        strict: descriptor.strict,
        active,
    }
}

pub(super) fn summarize_categories(descriptors: &[ToolDescriptor]) -> Vec<ToolCategorySummary> {
    let mut categories = BTreeMap::<String, Vec<&ToolDescriptor>>::new();
    for descriptor in descriptors {
        categories
            .entry(descriptor.category.clone())
            .or_default()
            .push(descriptor);
    }

    categories
        .into_iter()
        .map(|(category, tools)| ToolCategorySummary {
            name: category.clone(),
            description: category_description(category.as_str()),
            examples: tools
                .iter()
                .take(3)
                .map(|tool| tool.name.clone())
                .collect::<Vec<_>>(),
            estimated_tool_count: tools.len(),
        })
        .collect()
}

fn category_description(category: &str) -> &'static str {
    match category {
        super::TOOL_CATEGORY_EXTERNAL => "Use external tools injected by the current runtime.",
        TOOL_CATEGORY_WORKSPACE => "Inspect workspace files and repository structure.",
        TOOL_CATEGORY_SKILLS => "Read repo-local SKILL.md instruction files.",
        TOOL_CATEGORY_MCP => "Call remote tools exposed by configured MCP servers.",
        TOOL_CATEGORY_DISCOVERY => "Discover tool categories, summaries, and schemas.",
        _ => "Miscellaneous tools.",
    }
}

fn list_tools_in_category(
    descriptors: &[ToolDescriptor],
    arguments: &Value,
) -> Result<LlmToolOutput, LlmClientError> {
    let category = required_string(arguments, "category")?;
    let matching = descriptors
        .iter()
        .filter(|descriptor| descriptor.category == category)
        .map(|descriptor| {
            json!({
                "name": descriptor.name,
                "description": descriptor.description,
                "when_to_use": descriptor.when_to_use,
                "origin": origin_label(&descriptor.origin),
            })
        })
        .collect::<Vec<_>>();

    if matching.is_empty() {
        return Err(LlmClientError::Tool(format!(
            "tool category '{category}' was not found"
        )));
    }

    Ok(LlmToolOutput::Json(json!(matching)))
}

fn get_tool_schema(
    descriptors: &[ToolDescriptor],
    arguments: &Value,
) -> Result<LlmToolOutput, LlmClientError> {
    let tool_name = required_string(arguments, "tool_name")?;
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.name == tool_name)
        .ok_or_else(|| LlmClientError::Tool(format!("tool '{tool_name}' was not found")))?;
    Ok(LlmToolOutput::Json(json!({
        "name": descriptor.name,
        "description": descriptor.description,
        "category": descriptor.category,
        "when_to_use": descriptor.when_to_use,
        "origin": origin_label(&descriptor.origin),
        "parameters": descriptor.parameters,
        "strict": descriptor.strict,
    })))
}

fn origin_label(origin: &ToolOrigin) -> String {
    match origin {
        ToolOrigin::Local => "local".to_string(),
        ToolOrigin::External => "external".to_string(),
        ToolOrigin::Mcp { server } => format!("mcp:{server}"),
    }
}

#[cfg(test)]
#[path = "discovery_tools/tests.rs"]
mod tests;
