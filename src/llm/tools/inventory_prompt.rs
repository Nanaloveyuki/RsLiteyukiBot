use crate::llm::skills::{SkillInfo, SkillManager};

use super::ToolDescriptor;
use super::discovery_tools::summarize_categories;

pub(super) fn build_runtime_inventory_prompt(
    descriptors: &[ToolDescriptor],
    skills: &[SkillInfo],
    warnings: &[String],
    skill_manager: &SkillManager,
) -> Option<String> {
    let category_summaries = summarize_categories(descriptors);
    let mut tool_lines = Vec::new();
    if !category_summaries.is_empty() {
        tool_lines.push(
            "Tool discovery helpers are available: `list_tool_categories`, `list_tools_in_category`, `get_tool_schema`."
                .to_string(),
        );
        tool_lines.push("Available executable tool categories:".to_string());
        for category in category_summaries {
            tool_lines.push(format!(
                "- {} ({} tools): {}",
                category.name, category.estimated_tool_count, category.description
            ));
        }
    }
    if !warnings.is_empty() {
        tool_lines.push("Runtime capability warnings this turn:".to_string());
        for warning in warnings {
            tool_lines.push(format!("- {warning}"));
        }
    }

    merge_system_prompt_sections(
        None,
        [
            (!tool_lines.is_empty())
                .then(|| tool_lines.join("\n"))
                .as_deref(),
            skill_manager.build_inventory_prompt(skills).as_deref(),
        ],
    )
}

pub(crate) fn merge_system_prompt_sections<'a>(
    base: Option<&'a str>,
    extra_sections: impl IntoIterator<Item = Option<&'a str>>,
) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(base) = base.map(str::trim).filter(|value| !value.is_empty()) {
        sections.push(base.to_string());
    }
    for section in extra_sections {
        if let Some(section) = section.map(str::trim).filter(|value| !value.is_empty()) {
            sections.push(section.to_string());
        }
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

#[cfg(test)]
#[path = "inventory_prompt/tests.rs"]
mod tests;
