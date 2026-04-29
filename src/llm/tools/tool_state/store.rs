use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::utils::config_path::resolve_preferred_tool_state_path;

use super::super::NON_TOGGLEABLE_TOOL_NAMES;
use super::storage::{persist_tool_state, read_tool_state_source, tool_state_backup_path};

#[derive(Debug, Clone, Default)]
pub(in super::super) struct ToolStateStore {
    path: PathBuf,
    tools: BTreeMap<String, bool>,
    warnings: Vec<String>,
    load_error: Option<String>,
}

impl ToolStateStore {
    pub(in super::super) fn from_default_path() -> Self {
        Self::from_config_path(resolve_preferred_tool_state_path())
    }

    pub(in super::super) fn from_config_path(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let backup_path = tool_state_backup_path(path.as_path());
        let mut warnings = Vec::new();

        match read_tool_state_source(path.as_path(), backup_path.as_path()) {
            Ok((document, source_warning)) => {
                if let Some(source_warning) = source_warning {
                    warnings.push(source_warning);
                }
                Self {
                    path,
                    tools: document.tools,
                    warnings,
                    load_error: None,
                }
            }
            Err(err) => Self {
                path,
                tools: BTreeMap::new(),
                warnings: vec![err.clone()],
                load_error: Some(err),
            },
        }
    }

    pub(in super::super) fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub(in super::super) fn warnings(&self) -> &[String] {
        self.warnings.as_slice()
    }

    pub(in super::super) fn is_active(&self, name: &str) -> bool {
        if NON_TOGGLEABLE_TOOL_NAMES
            .iter()
            .any(|tool_name| tool_name == &name)
        {
            return true;
        }
        self.tools.get(name).copied().unwrap_or(true)
    }

    pub(in super::super) fn set_active(&mut self, name: &str, active: bool) -> Result<(), String> {
        if let Some(err) = &self.load_error {
            return Err(err.clone());
        }
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("tool name should not be empty".to_string());
        }
        if !active
            && NON_TOGGLEABLE_TOOL_NAMES
                .iter()
                .any(|tool_name| tool_name == &trimmed)
        {
            return Err(format!(
                "tool '{trimmed}' is a required discovery helper and cannot be toggled"
            ));
        }

        let previous = self.tools.get(trimmed).copied();
        if active {
            self.tools.remove(trimmed);
        } else {
            self.tools.insert(trimmed.to_string(), false);
        }

        if let Err(err) = persist_tool_state(self.path.as_path(), &self.tools) {
            restore_tool_entry(&mut self.tools, trimmed, previous);
            return Err(err);
        }

        Ok(())
    }
}

fn restore_tool_entry(tools: &mut BTreeMap<String, bool>, name: &str, previous: Option<bool>) {
    if let Some(previous) = previous {
        tools.insert(name.to_string(), previous);
    } else {
        tools.remove(name);
    }
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
