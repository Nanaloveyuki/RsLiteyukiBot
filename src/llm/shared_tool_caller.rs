use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::llm::client::{LlmClientError, LlmToolOutput};
use crate::llm::tools::{WorkspaceReadOnlyToolCaller, WorkspaceReadOnlyToolName};

#[derive(Debug, Clone)]
pub(crate) struct SharedToolCaller {
    workspace_read_only: WorkspaceReadOnlyToolCaller,
}

impl SharedToolCaller {
    pub(crate) fn for_workspace(workspace_root: &Path) -> Self {
        Self {
            workspace_read_only: WorkspaceReadOnlyToolCaller::new(workspace_root),
        }
    }

    pub(crate) fn call_workspace_read_only(
        &self,
        tool: WorkspaceReadOnlyToolName,
        arguments: &Value,
    ) -> Result<LlmToolOutput, LlmClientError> {
        self.workspace_read_only.call(tool, arguments)
    }
}

impl From<PathBuf> for SharedToolCaller {
    fn from(workspace_root: PathBuf) -> Self {
        Self::for_workspace(workspace_root.as_path())
    }
}
