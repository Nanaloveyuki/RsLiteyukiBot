use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::llm::client::{LlmClientError, LlmFunctionTool, LlmToolOutput};

use super::mcp::{McpBoundTool, McpManager};
use super::skills::{SkillCatalogEntry, SkillInfo, SkillManager};

const TOOL_CATEGORY_DISCOVERY: &str = "tool_discovery";
const TOOL_CATEGORY_WORKSPACE: &str = "workspace";
const TOOL_CATEGORY_SKILLS: &str = "skills";
const TOOL_CATEGORY_MCP: &str = "mcp";

const DEFAULT_FILE_READ_MAX_CHARS: usize = 12_000;
const DEFAULT_FILE_LIST_MAX_DEPTH: usize = 4;
const DEFAULT_FILE_LIST_MAX_ENTRIES: usize = 120;
const MAX_FILE_READ_MAX_CHARS: usize = 32_000;
const MAX_FILE_LIST_MAX_DEPTH: usize = 8;
const MAX_FILE_LIST_MAX_ENTRIES: usize = 500;

const IGNORED_DIR_NAMES: &[&str] = &[
    ".git",
    ".venv",
    ".pnpm-store",
    "node_modules",
    "target",
    "target-codex",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolOrigin {
    Local,
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
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolCatalogSnapshot {
    pub(crate) tools: Vec<ToolCatalogEntry>,
    pub(crate) warnings: Vec<String>,
}

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
struct ManagedTool {
    descriptor: ToolDescriptor,
    tool: LlmFunctionTool,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolManager {
    workspace_root: PathBuf,
    skill_manager: SkillManager,
    mcp_manager: McpManager,
}

impl ToolManager {
    pub(crate) fn for_workspace(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            skill_manager: SkillManager::for_workspace(workspace_root),
            mcp_manager: McpManager::from_default_config(),
        }
    }

    pub(crate) fn for_current_workspace() -> Result<Self, String> {
        let canonical = resolve_workspace_root()?;
        Ok(Self::for_workspace(canonical.as_path()))
    }

    pub(crate) async fn build_runtime_bundle(
        &self,
        extra_tools: &[LlmFunctionTool],
    ) -> Result<CapabilityBundle, String> {
        let (skills, mut warnings) = match self.skill_manager.list_skills() {
            Ok(skills) => (skills, Vec::new()),
            Err(err) => (
                Vec::new(),
                vec![format!("skills inventory unavailable: {err}")],
            ),
        };
        let mut tools = self.build_local_execution_tools();
        let mcp_load = self.mcp_manager.load_tools().await;
        warnings.extend(mcp_load.warnings.clone());
        tools.extend(mcp_load.tools.into_iter().map(to_managed_mcp_tool));

        let execution_descriptors = tools
            .iter()
            .map(|tool| tool.descriptor.clone())
            .collect::<Vec<_>>();
        tools.extend(build_discovery_tools(execution_descriptors.clone()));

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

        Ok(CapabilityBundle {
            tools: merged_tools,
            system_prompt: build_runtime_inventory_prompt(
                execution_descriptors.as_slice(),
                skills.as_slice(),
                warnings.as_slice(),
                &self.skill_manager,
            ),
        })
    }

    #[allow(dead_code)]
    pub(crate) async fn describe_runtime_tools(&self) -> ToolCatalogSnapshot {
        let mut warnings = Vec::new();
        let mut tools = self.build_local_execution_tools();
        let mcp_load = self.mcp_manager.load_tools().await;
        warnings.extend(mcp_load.warnings);
        tools.extend(mcp_load.tools.into_iter().map(to_managed_mcp_tool));

        let execution_descriptors = tools
            .iter()
            .map(|tool| tool.descriptor.clone())
            .collect::<Vec<_>>();
        tools.extend(build_discovery_tools(execution_descriptors));

        ToolCatalogSnapshot {
            tools: tools
                .into_iter()
                .map(|tool| tool_descriptor_to_catalog_entry(tool.descriptor))
                .collect(),
            warnings,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn describe_skills(&self) -> SkillCatalogSnapshot {
        match self.skill_manager.list_skills() {
            Ok(skills) => SkillCatalogSnapshot {
                skills: self.skill_manager.build_catalog(skills.as_slice()),
                warnings: Vec::new(),
            },
            Err(err) => SkillCatalogSnapshot {
                skills: Vec::new(),
                warnings: vec![format!("skills inventory unavailable: {err}")],
            },
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn describe_mcp_servers(&self) -> super::mcp::McpServerCatalogSnapshot {
        self.mcp_manager.inspect_servers().await
    }

    fn build_local_execution_tools(&self) -> Vec<ManagedTool> {
        let mut tools = Vec::new();

        let workspace_root = self.workspace_root.clone();
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
                            "description": "Maximum recursion depth, default 4, max 8."
                        },
                        "max_entries": {
                            "type": "integer",
                            "description": "Maximum number of file paths to return, default 120, max 500."
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
                let workspace_root = workspace_root.clone();
                async move { list_workspace_files(workspace_root.as_path(), &arguments) }
            },
        ));

        let workspace_root = self.workspace_root.clone();
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
                            "description": "Maximum number of characters to return, default 12000, max 32000."
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
                let workspace_root = workspace_root.clone();
                async move { read_workspace_file(workspace_root.as_path(), &arguments) }
            },
        ));

        let skill_manager = self.skill_manager.clone();
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

fn new_managed_tool<F, Fut>(descriptor: ToolDescriptor, handler: F) -> ManagedTool
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

fn to_managed_mcp_tool(binding: McpBoundTool) -> ManagedTool {
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

fn build_discovery_tools(descriptors: Vec<ToolDescriptor>) -> Vec<ManagedTool> {
    let category_entries = summarize_categories(descriptors.as_slice());

    let descriptors_for_tools = descriptors.clone();
    let categories_for_list = category_entries.clone();
    let list_categories = new_managed_tool(
        ToolDescriptor {
            name: "list_tool_categories".to_string(),
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
        move |_| {
            let categories = categories_for_list.clone();
            async move { Ok(LlmToolOutput::Json(json!(categories))) }
        },
    );

    let descriptors_for_category = descriptors.clone();
    let list_in_category = new_managed_tool(
        ToolDescriptor {
            name: "list_tools_in_category".to_string(),
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
        move |arguments| {
            let descriptors = descriptors_for_category.clone();
            async move { list_tools_in_category(descriptors.as_slice(), &arguments) }
        },
    );

    let schema_descriptors = descriptors_for_tools.clone();
    let get_schema = new_managed_tool(
        ToolDescriptor {
            name: "get_tool_schema".to_string(),
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
        move |arguments| {
            let descriptors = schema_descriptors.clone();
            async move { get_tool_schema(descriptors.as_slice(), &arguments) }
        },
    );

    vec![list_categories, list_in_category, get_schema]
}

#[allow(dead_code)]
fn tool_descriptor_to_catalog_entry(descriptor: ToolDescriptor) -> ToolCatalogEntry {
    ToolCatalogEntry {
        name: descriptor.name,
        description: descriptor.description,
        parameters: descriptor.parameters,
        category: descriptor.category,
        when_to_use: descriptor.when_to_use,
        origin: origin_label(&descriptor.origin),
        strict: descriptor.strict,
    }
}

fn build_runtime_inventory_prompt(
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
                category["name"].as_str().unwrap_or_default(),
                category["estimated_tool_count"]
                    .as_u64()
                    .unwrap_or_default(),
                category["description"].as_str().unwrap_or_default()
            ));
        }
    }
    if !warnings.is_empty() {
        tool_lines.push("Unavailable MCP sources this turn:".to_string());
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

fn resolve_workspace_root() -> Result<PathBuf, String> {
    if let Some(explicit_root) = std::env::var_os("LY_WORKSPACE_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Ok(fs::canonicalize(&explicit_root).unwrap_or(explicit_root));
    }

    let current = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current workspace: {err}"))?;
    let canonical = fs::canonicalize(&current).unwrap_or(current);
    let mut cursor = canonical.as_path();
    loop {
        if cursor.join(".git").exists() || cursor.join("Cargo.toml").is_file() {
            return Ok(cursor.to_path_buf());
        }
        let Some(parent) = cursor.parent() else {
            return Ok(canonical);
        };
        cursor = parent;
    }
}

fn summarize_categories(descriptors: &[ToolDescriptor]) -> Vec<Value> {
    let mut categories = BTreeMap::<String, Vec<&ToolDescriptor>>::new();
    for descriptor in descriptors {
        categories
            .entry(descriptor.category.clone())
            .or_default()
            .push(descriptor);
    }

    categories
        .into_iter()
        .map(|(category, tools)| {
            json!({
                "name": category,
                "description": category_description(category.as_str()),
                "examples": tools
                    .iter()
                    .take(3)
                    .map(|tool| tool.name.clone())
                    .collect::<Vec<_>>(),
                "estimated_tool_count": tools.len(),
            })
        })
        .collect()
}

fn category_description(category: &str) -> &'static str {
    match category {
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
        ToolOrigin::Mcp { server } => format!("mcp:{server}"),
    }
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

fn list_workspace_files(
    workspace_root: &Path,
    arguments: &Value,
) -> Result<LlmToolOutput, LlmClientError> {
    let path = optional_string(arguments, "path")?
        .map(|value| resolve_workspace_path(workspace_root, value.as_str()))
        .transpose()?
        .unwrap_or_else(|| workspace_root.to_path_buf());
    let max_depth = optional_usize(arguments, "max_depth")?
        .unwrap_or(DEFAULT_FILE_LIST_MAX_DEPTH)
        .clamp(0, MAX_FILE_LIST_MAX_DEPTH);
    let max_entries = optional_usize(arguments, "max_entries")?
        .unwrap_or(DEFAULT_FILE_LIST_MAX_ENTRIES)
        .clamp(1, MAX_FILE_LIST_MAX_ENTRIES);
    ensure_resolved_path_within_workspace(workspace_root, path.as_path())
        .map_err(LlmClientError::Tool)?;

    let mut files = Vec::new();
    collect_workspace_files(
        workspace_root,
        path.as_path(),
        0,
        max_depth,
        max_entries,
        &mut files,
    )
    .map_err(LlmClientError::Tool)?;

    Ok(LlmToolOutput::Json(json!({
        "root": display_path(path.as_path(), workspace_root),
        "files": files,
        "count": files.len(),
    })))
}

fn read_workspace_file(
    workspace_root: &Path,
    arguments: &Value,
) -> Result<LlmToolOutput, LlmClientError> {
    let path =
        resolve_workspace_path(workspace_root, required_string(arguments, "path")?.as_str())?;
    ensure_resolved_path_within_workspace(workspace_root, path.as_path())
        .map_err(LlmClientError::Tool)?;
    if !path.is_file() {
        return Err(LlmClientError::Tool(format!(
            "workspace path '{}' is not a file",
            display_path(path.as_path(), workspace_root)
        )));
    }

    let max_chars = optional_usize(arguments, "max_chars")?
        .unwrap_or(DEFAULT_FILE_READ_MAX_CHARS)
        .clamp(256, MAX_FILE_READ_MAX_CHARS);
    let start_line = optional_usize(arguments, "start_line")?.unwrap_or(1).max(1);
    let content_bytes = fs::read(&path).map_err(|err| {
        LlmClientError::Tool(format!(
            "failed to read file '{}': {err}",
            display_path(path.as_path(), workspace_root)
        ))
    })?;
    let text = String::from_utf8_lossy(&content_bytes).into_owned();
    let lines = text.lines().collect::<Vec<_>>();
    let total_lines = lines.len().max(1);
    let end_line = optional_usize(arguments, "end_line")?
        .unwrap_or(total_lines)
        .clamp(start_line, total_lines);
    let start_index = start_line.saturating_sub(1).min(lines.len());
    let end_index = end_line.min(lines.len());

    let mut rendered = format!("File: {}\n", display_path(path.as_path(), workspace_root));
    if lines.is_empty() {
        rendered.push_str("[empty file]");
        return Ok(LlmToolOutput::Text(rendered));
    }

    let mut truncated = false;
    for (offset, line) in lines[start_index..end_index].iter().enumerate() {
        let line_no = start_index + offset + 1;
        let next = format!("{line_no:>4} | {line}\n");
        if rendered.chars().count() + next.chars().count() > max_chars {
            truncated = true;
            break;
        }
        rendered.push_str(next.as_str());
    }
    if truncated {
        rendered.push_str("[truncated]\n");
    }
    Ok(LlmToolOutput::Text(rendered.trim_end().to_string()))
}

fn collect_workspace_files(
    workspace_root: &Path,
    path: &Path,
    depth: usize,
    max_depth: usize,
    max_entries: usize,
    output: &mut Vec<String>,
) -> Result<(), String> {
    if output.len() >= max_entries {
        return Ok(());
    }
    if path.is_file() {
        output.push(display_path(path, workspace_root));
        return Ok(());
    }
    if !path.is_dir() {
        return Err(format!(
            "workspace path '{}' is not a file or directory",
            display_path(path, workspace_root)
        ));
    }

    let mut entries = fs::read_dir(path)
        .map_err(|err| format!("failed to read directory '{}': {err}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("failed to enumerate '{}': {err}", path.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if output.len() >= max_entries {
            break;
        }

        let entry_path = entry.path();
        if !path_is_within_workspace(workspace_root, entry_path.as_path()) {
            continue;
        }
        let Some(name) = entry_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if entry_path.is_dir() {
            if depth >= max_depth || IGNORED_DIR_NAMES.iter().any(|ignored| ignored == &name) {
                continue;
            }
            collect_workspace_files(
                workspace_root,
                entry_path.as_path(),
                depth + 1,
                max_depth,
                max_entries,
                output,
            )?;
        } else if entry_path.is_file() {
            output.push(display_path(entry_path.as_path(), workspace_root));
        }
    }

    Ok(())
}

fn resolve_workspace_path(
    workspace_root: &Path,
    raw_path: &str,
) -> Result<PathBuf, LlmClientError> {
    let sanitized = sanitize_relative_path(raw_path).map_err(LlmClientError::Tool)?;
    Ok(workspace_root.join(sanitized))
}

fn ensure_resolved_path_within_workspace(workspace_root: &Path, path: &Path) -> Result<(), String> {
    if path_is_within_workspace(workspace_root, path) {
        Ok(())
    } else {
        Err(format!(
            "workspace path '{}' escapes the workspace root",
            display_path(path, workspace_root)
        ))
    }
}

fn path_is_within_workspace(workspace_root: &Path, path: &Path) -> bool {
    let canonical_root =
        fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    let canonical_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical_path.starts_with(&canonical_root)
}

fn sanitize_relative_path(raw_path: &str) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("path cannot be empty".to_string());
    }

    let input = Path::new(trimmed);
    if input.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }

    let mut sanitized = PathBuf::new();
    for component in input.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => sanitized.push(part),
            Component::ParentDir => {
                return Err("parent-directory segments are not allowed".to_string());
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err("absolute paths are not allowed".to_string());
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(sanitized)
    }
}

fn display_path(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn required_string(arguments: &Value, key: &str) -> Result<String, LlmClientError> {
    optional_string(arguments, key)?
        .ok_or_else(|| LlmClientError::Tool(format!("missing required argument '{key}'")))
}

fn optional_string(arguments: &Value, key: &str) -> Result<Option<String>, LlmClientError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.trim().to_string())),
        Some(_) => Err(LlmClientError::Tool(format!(
            "argument '{key}' must be a string"
        ))),
    }
}

fn optional_usize(arguments: &Value, key: &str) -> Result<Option<usize>, LlmClientError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_u64()
            .map(|value| value as usize)
            .map(Some)
            .ok_or_else(|| {
                LlmClientError::Tool(format!("argument '{key}' must be a non-negative integer"))
            }),
        Some(_) => Err(LlmClientError::Tool(format!(
            "argument '{key}' must be a non-negative integer"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn isolated_manager_for_workspace(workspace_root: &Path) -> ToolManager {
        ToolManager {
            workspace_root: workspace_root.to_path_buf(),
            skill_manager: SkillManager::for_workspace(workspace_root),
            mcp_manager: McpManager::from_config_path(temp_path("missing-mcp-config.json")),
        }
    }

    #[test]
    fn sanitize_relative_path_rejects_parent_dirs() {
        let error = sanitize_relative_path("../secret").expect_err("path should be rejected");
        assert!(error.contains("parent-directory"));
    }

    #[test]
    fn merge_system_prompt_sections_skips_empty_values() {
        let merged = merge_system_prompt_sections(Some("base"), [None, Some("extra")]);
        assert_eq!(merged.as_deref(), Some("base\n\nextra"));
    }

    #[test]
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

    fn temp_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("liteyuki-tools-test-{label}-{unique}"))
    }
}
