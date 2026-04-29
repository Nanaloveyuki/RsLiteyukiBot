#[path = "tools/discovery_tools.rs"]
mod discovery_tools;
#[path = "tools/inventory_prompt.rs"]
mod inventory_prompt;
#[path = "tools/local_execution_tools.rs"]
mod local_execution_tools;
#[path = "tools/runtime_inventory.rs"]
mod runtime_inventory;
#[cfg(test)]
#[path = "tools/tests.rs"]
mod tests;
#[path = "tools/tool_arguments.rs"]
mod tool_arguments;
#[path = "tools/tool_state.rs"]
mod tool_state;
#[path = "tools/tool_types.rs"]
mod tool_types;
#[path = "tools/workspace_access.rs"]
mod workspace_access;

use self::discovery_tools::{build_discovery_tools, tool_descriptor_to_catalog_entry};
use self::inventory_prompt::build_runtime_inventory_prompt;
use self::local_execution_tools::build_local_execution_tools;
use self::runtime_inventory::{
    RuntimeToolInventory, collect_execution_descriptors, collect_external_descriptors,
    merge_runtime_tools, retain_active_tools, to_managed_mcp_tool,
};
use self::tool_state::ToolStateStore;
use self::tool_types::{
    CapabilityBundle, ManagedTool, SkillCatalogSnapshot, ToolCatalogEntry, ToolCatalogSnapshot,
};
use self::workspace_access::resolve_workspace_root;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::mcp::McpManager;
use super::skills::SkillManager;
use crate::llm::client::LlmFunctionTool;

const TOOL_CATEGORY_DISCOVERY: &str = "tool_discovery";
const TOOL_CATEGORY_EXTERNAL: &str = "external_runtime";
const TOOL_CATEGORY_WORKSPACE: &str = "workspace";
const TOOL_CATEGORY_SKILLS: &str = "skills";
const TOOL_CATEGORY_MCP: &str = "mcp";
pub(crate) const LIST_TOOL_CATEGORIES_TOOL_NAME: &str = "list_tool_categories";
pub(crate) const LIST_TOOLS_IN_CATEGORY_TOOL_NAME: &str = "list_tools_in_category";
pub(crate) const GET_TOOL_SCHEMA_TOOL_NAME: &str = "get_tool_schema";
pub(crate) const NON_TOGGLEABLE_TOOL_NAMES: &[&str] = &[
    LIST_TOOL_CATEGORIES_TOOL_NAME,
    LIST_TOOLS_IN_CATEGORY_TOOL_NAME,
    GET_TOOL_SCHEMA_TOOL_NAME,
];

pub(crate) const DEFAULT_FILE_READ_MAX_CHARS: usize = 12_000;
pub(crate) const DEFAULT_FILE_LIST_MAX_DEPTH: usize = 4;
pub(crate) const DEFAULT_FILE_LIST_MAX_ENTRIES: usize = 120;
pub(crate) const MAX_FILE_READ_MAX_CHARS: usize = 32_000;
pub(crate) const MAX_FILE_LIST_MAX_DEPTH: usize = 8;
pub(crate) const MAX_FILE_LIST_MAX_ENTRIES: usize = 500;

const IGNORED_DIR_NAMES: &[&str] = &[
    ".git",
    ".venv",
    ".pnpm-store",
    "node_modules",
    "target",
    "target-codex",
];

#[derive(Debug, Clone)]
pub(crate) struct ToolManager {
    workspace_root: PathBuf,
    skill_manager: SkillManager,
    mcp_manager: McpManager,
    tool_state: ToolStateStore,
}

pub(crate) use self::inventory_prompt::merge_system_prompt_sections;
pub(crate) use self::tool_types::{ToolDescriptor, ToolOrigin};

impl ToolManager {
    pub(crate) fn for_workspace(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            skill_manager: SkillManager::for_workspace(workspace_root),
            mcp_manager: McpManager::from_default_config(),
            tool_state: ToolStateStore::from_default_path(),
        }
    }

    pub(crate) fn for_current_workspace() -> Result<Self, String> {
        let canonical = resolve_workspace_root()?;
        Ok(Self::for_workspace(canonical.as_path()))
    }

    #[cfg(test)]
    pub(super) fn for_test_workspace(
        workspace_root: &Path,
        tool_state_path: &Path,
        mcp_config_path: &Path,
    ) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            skill_manager: SkillManager::for_workspace(workspace_root),
            mcp_manager: McpManager::from_config_path(mcp_config_path),
            tool_state: ToolStateStore::from_config_path(tool_state_path),
        }
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
        let RuntimeToolInventory {
            mut tools,
            warnings: runtime_warnings,
        } = self.collect_runtime_tool_inventory().await;
        warnings.extend(runtime_warnings);

        let execution_descriptors =
            collect_execution_descriptors(tools.as_slice(), &self.tool_state, extra_tools);
        retain_active_tools(&mut tools, &self.tool_state);
        tools.extend(
            build_discovery_tools(execution_descriptors.clone())
                .into_iter()
                .filter(|tool| self.tool_state.is_active(tool.descriptor.name.as_str())),
        );

        Ok(CapabilityBundle {
            tools: merge_runtime_tools(tools.as_slice(), extra_tools)?,
            system_prompt: build_runtime_inventory_prompt(
                execution_descriptors.as_slice(),
                skills.as_slice(),
                warnings.as_slice(),
                &self.skill_manager,
            ),
        })
    }

    // 外部调用
    #[allow(dead_code)]
    pub(crate) async fn describe_runtime_tools(
        &self,
        extra_tools: &[LlmFunctionTool],
    ) -> ToolCatalogSnapshot {
        let RuntimeToolInventory {
            mut tools,
            mut warnings,
        } = self.collect_runtime_tool_inventory().await;
        let external_descriptors = collect_external_descriptors(extra_tools);

        let mut execution_descriptors = Vec::new();
        let mut execution_names = HashSet::new();
        for tool in tools
            .iter()
            .filter(|tool| self.tool_state.is_active(tool.descriptor.name.as_str()))
        {
            push_unique_descriptor(
                &mut execution_descriptors,
                &mut execution_names,
                &mut warnings,
                tool.descriptor.clone(),
                "runtime execution tool",
            );
        }
        for descriptor in external_descriptors.clone() {
            push_unique_descriptor(
                &mut execution_descriptors,
                &mut execution_names,
                &mut warnings,
                descriptor,
                "external runtime tool",
            );
        }
        tools.extend(build_discovery_tools(execution_descriptors));

        let mut catalog_tools = Vec::new();
        let mut catalog_names = HashSet::new();
        for tool in tools {
            let active = self.tool_state.is_active(tool.descriptor.name.as_str());
            push_unique_catalog_entry(
                &mut catalog_tools,
                &mut catalog_names,
                &mut warnings,
                tool.descriptor,
                active,
                "runtime catalog entry",
            );
        }
        for descriptor in external_descriptors {
            push_unique_catalog_entry(
                &mut catalog_tools,
                &mut catalog_names,
                &mut warnings,
                descriptor,
                true,
                "external runtime catalog entry",
            );
        }

        ToolCatalogSnapshot {
            tools: catalog_tools,
            warnings,
        }
    }

    // 外部调用
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

    // 外部调用
    #[allow(dead_code)]
    pub(crate) async fn describe_mcp_servers(&self) -> super::mcp::McpServerCatalogSnapshot {
        self.mcp_manager.inspect_servers().await
    }

    // 外部调用
    #[allow(dead_code)]
    pub(crate) fn set_tool_active(&mut self, name: &str, active: bool) -> Result<(), String> {
        self.tool_state.set_active(name, active)
    }

    // 外部调用
    #[allow(dead_code)]
    pub(crate) fn tool_state_path(&self) -> &Path {
        self.tool_state.path()
    }

    fn build_local_execution_tools(&self) -> Vec<ManagedTool> {
        build_local_execution_tools(self.workspace_root.as_path(), self.skill_manager.clone())
    }

    async fn collect_runtime_tool_inventory(&self) -> RuntimeToolInventory {
        let mut warnings = self.tool_state.warnings().to_vec();
        let mut tools = self.build_local_execution_tools();
        let mcp_load = self.mcp_manager.load_tools().await;
        warnings.extend(mcp_load.warnings);
        tools.extend(mcp_load.tools.into_iter().map(to_managed_mcp_tool));

        RuntimeToolInventory { tools, warnings }
    }
}

fn push_unique_descriptor(
    descriptors: &mut Vec<ToolDescriptor>,
    seen_names: &mut HashSet<String>,
    warnings: &mut Vec<String>,
    descriptor: ToolDescriptor,
    source: &str,
) {
    if !seen_names.insert(descriptor.name.clone()) {
        warnings.push(format!(
            "duplicate tool name '{}' skipped while building {source}",
            descriptor.name
        ));
        return;
    }

    descriptors.push(descriptor);
}

fn push_unique_catalog_entry(
    catalog: &mut Vec<ToolCatalogEntry>,
    seen_names: &mut HashSet<String>,
    warnings: &mut Vec<String>,
    descriptor: ToolDescriptor,
    active: bool,
    source: &str,
) {
    if !seen_names.insert(descriptor.name.clone()) {
        warnings.push(format!(
            "duplicate tool name '{}' skipped while building {source}",
            descriptor.name
        ));
        return;
    }

    catalog.push(tool_descriptor_to_catalog_entry(descriptor, active));
}
