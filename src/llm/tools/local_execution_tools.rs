use std::path::Path;

use serde_json::{Value, json};

use crate::llm::client::{LlmClientError, LlmToolOutput};
use crate::llm::skills::SkillManager;

use super::tool_arguments::{optional_usize, required_string};
use super::tool_types::{ManagedTool, new_managed_tool};
use super::workspace_access::{list_workspace_files, read_workspace_file};
use super::{
    DEFAULT_FILE_LIST_MAX_DEPTH, DEFAULT_FILE_LIST_MAX_ENTRIES, DEFAULT_FILE_READ_MAX_CHARS,
    MAX_FILE_LIST_MAX_DEPTH, MAX_FILE_LIST_MAX_ENTRIES, MAX_FILE_READ_MAX_CHARS,
    TOOL_CATEGORY_SKILLS, TOOL_CATEGORY_WORKSPACE, ToolDescriptor, ToolOrigin,
};

pub(super) fn build_local_execution_tools(
    workspace_root: &Path,
    skill_manager: SkillManager,
) -> Vec<ManagedTool> {
    let mut tools = Vec::new();
    let max_depth_description = format!(
        "Maximum recursion depth, default {DEFAULT_FILE_LIST_MAX_DEPTH}, max {MAX_FILE_LIST_MAX_DEPTH}."
    );
    let max_entries_description = format!(
        "Maximum number of file paths to return, default {DEFAULT_FILE_LIST_MAX_ENTRIES}, max {MAX_FILE_LIST_MAX_ENTRIES}."
    );
    let max_chars_description = format!(
        "Maximum number of characters to return, default {DEFAULT_FILE_READ_MAX_CHARS}, max {MAX_FILE_READ_MAX_CHARS}."
    );

    let workspace_root = workspace_root.to_path_buf();
    let list_workspace_root = workspace_root.clone();
    tools.push(new_managed_tool(
        ToolDescriptor {
            name: "workspace_list_files".to_string(),
            description: "List files under a workspace-relative directory.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative directory or file path. Defaults to the workspace root."
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": max_depth_description
                    },
                    "max_entries": {
                        "type": "integer",
                        "description": max_entries_description
                    }
                },
                "additionalProperties": false
            }),
            category: TOOL_CATEGORY_WORKSPACE.to_string(),
            when_to_use: "Use when you need to discover repository files before reading them."
                .to_string(),
            origin: ToolOrigin::Local,
            strict: true,
        },
        move |arguments| {
            let workspace_root = list_workspace_root.clone();
            async move { list_workspace_files(workspace_root.as_path(), &arguments) }
        },
    ));

    let read_workspace_root = workspace_root;
    tools.push(new_managed_tool(
        ToolDescriptor {
            name: "workspace_read_file".to_string(),
            description: "Read a UTF-8 workspace file with optional line slicing.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative file path."
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "1-based inclusive start line."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "1-based inclusive end line."
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": max_chars_description
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            category: TOOL_CATEGORY_WORKSPACE.to_string(),
            when_to_use:
                "Use when you already know which repository file or SKILL.md you need to inspect."
                    .to_string(),
            origin: ToolOrigin::Local,
            strict: true,
        },
        move |arguments| {
            let workspace_root = read_workspace_root.clone();
            async move { read_workspace_file(workspace_root.as_path(), &arguments) }
        },
    ));

    tools.push(new_managed_tool(
        ToolDescriptor {
            name: "read_skill_document".to_string(),
            description: "Read the SKILL.md file for a repo-local skill by exact skill name."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Exact skill directory name."
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum number of characters to return."
                    }
                },
                "required": ["skill_name"],
                "additionalProperties": false
            }),
            category: TOOL_CATEGORY_SKILLS.to_string(),
            when_to_use:
                "Use when the skill inventory suggests a repo-local skill is relevant to the task."
                    .to_string(),
            origin: ToolOrigin::Local,
            strict: true,
        },
        move |arguments| {
            let skill_manager = skill_manager.clone();
            async move { read_skill_document(skill_manager, &arguments) }
        },
    ));

    tools
}
fn read_skill_document(
    skill_manager: SkillManager,
    arguments: &Value,
) -> Result<LlmToolOutput, LlmClientError> {
    let skill_name = required_string(arguments, "skill_name")?;
    let max_chars = optional_usize(arguments, "max_chars")?;
    skill_manager
        .read_skill_document(skill_name.as_str(), max_chars)
        .map(LlmToolOutput::Text)
        .map_err(LlmClientError::Tool)
}

#[cfg(test)]
#[path = "local_execution_tools/tests.rs"]
mod tests;
