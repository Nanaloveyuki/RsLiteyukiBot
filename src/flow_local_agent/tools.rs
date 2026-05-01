use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::app_config::FlowLocalAgentRuntimeConfig;

use super::protocol::FlowLocalAgentRequest;

const MAX_READ_FILE_BYTES: usize = 100_000;

#[derive(Debug, Clone)]
pub(crate) struct FlowLocalAgentToolExecutor {
    allowed_tools: Vec<String>,
    path_resolver: FlowLocalPathResolver,
}

#[derive(Debug, Clone)]
struct FlowLocalPathResolver {
    base_dir: PathBuf,
    home_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FlowLocalFileEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    size: u64,
}

impl FlowLocalAgentToolExecutor {
    pub(crate) fn from_runtime_config(runtime_config: &FlowLocalAgentRuntimeConfig) -> Self {
        Self {
            allowed_tools: runtime_config.allowed_tools.clone(),
            path_resolver: FlowLocalPathResolver::new(runtime_config.workspace_root.clone()),
        }
    }

    pub(crate) fn execute(&self, request: &FlowLocalAgentRequest) -> Result<String, String> {
        if !self
            .allowed_tools
            .iter()
            .any(|tool| tool == request.tool.as_str())
        {
            return Err(format!(
                "tool '{}' is not allowed by flow_local_agent.allowed_tools",
                request.tool
            ));
        }

        match request.tool.as_str() {
            "list_files" => self.list_files(&request.args),
            "read_file" => self.read_file(&request.args),
            other => Err(format!(
                "unsupported flow local agent tool during read-only phase: {other}"
            )),
        }
    }

    fn list_files(&self, arguments: &serde_json::Value) -> Result<String, String> {
        let path = arguments
            .get("path")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(".");
        let path = self.path_resolver.resolve(path)?;
        if !path.is_dir() {
            return Err(format!("path '{}' is not a directory", path.display()));
        }

        let mut entries = fs::read_dir(&path)
            .map_err(|err| format!("failed to read directory '{}': {err}", path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("failed to enumerate '{}': {err}", path.display()))?;
        entries.sort_by_key(|entry| entry.file_name());

        let output = entries
            .into_iter()
            .filter_map(|entry| to_file_entry(entry.path().as_path()))
            .collect::<Vec<_>>();
        serde_json::to_string(&output)
            .map_err(|err| format!("failed to serialize list_files result: {err}"))
    }

    fn read_file(&self, arguments: &serde_json::Value) -> Result<String, String> {
        let path = arguments
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "missing required string field 'path'".to_string())?;
        let path = self.path_resolver.resolve(path)?;
        if !path.is_file() {
            return Err(format!("path '{}' is not a file", path.display()));
        }

        let content = fs::read(&path)
            .map_err(|err| format!("failed to read file '{}': {err}", path.display()))?;
        Ok(
            String::from_utf8_lossy(&content[..content.len().min(MAX_READ_FILE_BYTES)])
                .into_owned(),
        )
    }
}

impl FlowLocalPathResolver {
    fn new(base_dir: Option<PathBuf>) -> Self {
        let base_dir = base_dir.unwrap_or_else(|| PathBuf::from("."));
        Self {
            base_dir,
            home_dir: crate::utils::config_path::resolve_user_home_dir(),
        }
    }

    fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("path cannot be empty".to_string());
        }

        if let Some(stripped) = trimmed.strip_prefix('~') {
            let home = self.home_dir.as_ref().ok_or_else(|| {
                "cannot expand '~' because home directory is unavailable".to_string()
            })?;
            if stripped.is_empty() {
                return Ok(home.clone());
            }
            let suffix = stripped.trim_start_matches(['/', '\\']);
            return Ok(home.join(suffix));
        }

        let input = PathBuf::from(trimmed);
        if input.is_absolute() {
            return Ok(input);
        }

        Ok(self.base_dir.join(input))
    }
}

fn to_file_entry(path: &Path) -> Option<FlowLocalFileEntry> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let metadata = fs::metadata(path).ok();
    let (kind, size) = match metadata {
        Some(metadata) if metadata.is_dir() => ("dir".to_string(), metadata.len()),
        Some(metadata) if metadata.is_file() => ("file".to_string(), metadata.len()),
        Some(metadata) if metadata.file_type().is_symlink() => {
            ("unknown".to_string(), metadata.len())
        }
        Some(metadata) => ("unknown".to_string(), metadata.len()),
        None => ("unknown".to_string(), 0),
    };
    Some(FlowLocalFileEntry { name, kind, size })
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn executor_lists_entries_with_upstream_shape() {
        let workspace_root = temp_path("flow-list");
        fs::create_dir_all(workspace_root.join("docs")).expect("workspace root should exist");
        fs::write(workspace_root.join("docs").join("plan.md"), "# plan\n")
            .expect("file should be written");

        let executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config(
            Some(workspace_root.clone()),
            vec!["list_files".to_string(), "read_file".to_string()],
        ));
        let output = executor
            .execute(&FlowLocalAgentRequest {
                id: "req-1".to_string(),
                tool: "list_files".to_string(),
                args: json!({"path": "docs"}),
            })
            .expect("list_files should succeed");

        let entries = serde_json::from_str::<serde_json::Value>(&output)
            .expect("output should be JSON array");
        let items = entries.as_array().expect("entries should be an array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "plan.md");
        assert_eq!(items[0]["type"], "file");

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn executor_reads_absolute_file_path() {
        let workspace_root = temp_path("flow-absolute-read");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");
        let file = workspace_root.join("notes.txt");
        fs::write(&file, "hello flow\n").expect("file should be written");

        let executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config(
            None,
            vec!["read_file".to_string()],
        ));
        let output = executor
            .execute(&FlowLocalAgentRequest {
                id: "req-2".to_string(),
                tool: "read_file".to_string(),
                args: json!({"path": file.display().to_string()}),
            })
            .expect("read_file should succeed");

        assert_eq!(output, "hello flow\n");

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn executor_expands_home_directory_for_list_files() {
        let home = temp_path("flow-home");
        let docs = home.join("agent-home-docs");
        fs::create_dir_all(&docs).expect("home docs should exist");
        fs::write(docs.join("guide.md"), "# guide\n").expect("guide should be written");

        let previous_userprofile = std::env::var("USERPROFILE").ok();
        unsafe {
            std::env::set_var("USERPROFILE", &home);
        }

        let executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config(
            None,
            vec!["list_files".to_string()],
        ));
        let output = executor
            .execute(&FlowLocalAgentRequest {
                id: "req-3".to_string(),
                tool: "list_files".to_string(),
                args: json!({"path": "~/agent-home-docs"}),
            })
            .expect("list_files should expand home");

        let entries = serde_json::from_str::<serde_json::Value>(&output)
            .expect("output should be JSON array");
        let items = entries.as_array().expect("entries should be an array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "guide.md");

        match previous_userprofile {
            Some(value) => unsafe { std::env::set_var("USERPROFILE", value) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn executor_rejects_disallowed_tool() {
        let workspace_root = temp_path("flow-disallowed");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");

        let executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config(
            Some(workspace_root.clone()),
            vec!["read_file".to_string()],
        ));
        let error = executor
            .execute(&FlowLocalAgentRequest {
                id: "req-4".to_string(),
                tool: "list_files".to_string(),
                args: json!({}),
            })
            .expect_err("list_files should be rejected when not allowed");

        assert!(error.contains("not allowed"));

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn executor_rejects_unsupported_write_phase_tools() {
        let workspace_root = temp_path("flow-unsupported");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");

        let executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config(
            Some(workspace_root.clone()),
            vec!["write_file".to_string()],
        ));
        let error = executor
            .execute(&FlowLocalAgentRequest {
                id: "req-5".to_string(),
                tool: "write_file".to_string(),
                args: json!({"path": "notes.txt", "content": "demo"}),
            })
            .expect_err("write_file should remain unsupported in read-only phase");

        assert!(error.contains("read-only phase"));

        let _ = fs::remove_dir_all(workspace_root);
    }

    fn runtime_config(
        workspace_root: Option<PathBuf>,
        allowed_tools: Vec<String>,
    ) -> FlowLocalAgentRuntimeConfig {
        FlowLocalAgentRuntimeConfig {
            enabled: true,
            base_url: Some("https://flow.liteyuki.org".to_string()),
            token: Some("lys_test".to_string()),
            device_id: Some("device-1".to_string()),
            device_name: Some("Test Device".to_string()),
            auto_connect: true,
            allowed_tools,
            workspace_root,
            command_timeout_ms: 30_000,
            approval_policy: "prompt".to_string(),
        }
    }

    fn temp_path(label: &str) -> PathBuf {
        let process_id = std::process::id();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "liteyuki-flow-local-agent-tools-test-{label}-{process_id}-{unique}"
        ))
    }
}
