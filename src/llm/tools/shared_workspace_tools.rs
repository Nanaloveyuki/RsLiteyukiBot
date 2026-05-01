use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::llm::client::{LlmClientError, LlmToolOutput};

use super::workspace_access::{list_workspace_files, read_workspace_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceReadOnlyToolName {
    ListFiles,
    ReadFile,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceReadOnlyToolCaller {
    workspace_root: PathBuf,
}

impl WorkspaceReadOnlyToolCaller {
    pub(crate) fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    pub(crate) fn call(
        &self,
        tool: WorkspaceReadOnlyToolName,
        arguments: &Value,
    ) -> Result<LlmToolOutput, LlmClientError> {
        match tool {
            WorkspaceReadOnlyToolName::ListFiles => {
                list_workspace_files(self.workspace_root.as_path(), arguments)
            }
            WorkspaceReadOnlyToolName::ReadFile => {
                read_workspace_file(self.workspace_root.as_path(), arguments)
            }
        }
    }
}

#[cfg(test)]
#[path = "shared_workspace_tools/tests.rs"]
mod tests;
