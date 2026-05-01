use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use crate::app_config::FlowLocalAgentRuntimeConfig;
use liteyukibot_core::{LogLevel, emit_console_log};

use super::protocol::FlowLocalAgentRequest;

const MAX_READ_FILE_BYTES: usize = 100_000;
const MAX_COMMAND_OUTPUT_BYTES: usize = 50_000;

#[derive(Debug, Clone)]
pub(crate) struct FlowLocalAgentToolExecutor {
    allowed_tools: Vec<String>,
    path_resolver: FlowLocalPathResolver,
    command_timeout_ms: u64,
}

#[derive(Debug, Clone)]
struct FlowLocalPathResolver {
    base_dir: PathBuf,
    home_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            command_timeout_ms: runtime_config.command_timeout_ms.max(10),
        }
    }

    pub(crate) async fn execute(&self, request: &FlowLocalAgentRequest) -> Result<String, String> {
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
            "list_files" => self.list_files(request),
            "read_file" => self.read_file(request),
            "write_file" => self.write_file(request),
            "run_command" => self.run_command(request).await,
            other => Err(format!("unsupported flow local agent tool: {other}")),
        }
    }

    fn list_files(&self, request: &FlowLocalAgentRequest) -> Result<String, String> {
        let path = request
            .args
            .get("path")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(".");
        let path = self.path_resolver.resolve(path)?;
        if !path.is_dir() {
            return Err(format!("path '{}' is not a directory", path.display()));
        }

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent.tool",
            format!(
                "list_files request (id={}, path={})",
                request.id,
                path.display()
            ),
        );

        let mut entries = fs::read_dir(&path)
            .map_err(|err| format!("failed to read directory '{}': {err}", path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("failed to enumerate '{}': {err}", path.display()))?;
        entries.sort_by_key(|entry| entry.file_name());

        let output = entries
            .into_iter()
            .filter_map(|entry| to_file_entry(entry.path().as_path()))
            .collect::<Vec<_>>();

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent.tool",
            format!(
                "list_files completed (id={}, path={}, entries={})",
                request.id,
                path.display(),
                output.len()
            ),
        );

        serde_json::to_string(&output)
            .map_err(|err| format!("failed to serialize list_files result: {err}"))
    }

    fn read_file(&self, request: &FlowLocalAgentRequest) -> Result<String, String> {
        let path = request
            .args
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "missing required string field 'path'".to_string())?;
        let path = self.path_resolver.resolve(path)?;
        if !path.is_file() {
            return Err(format!("path '{}' is not a file", path.display()));
        }

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent.tool",
            format!(
                "read_file request (id={}, path={})",
                request.id,
                path.display()
            ),
        );

        let content = fs::read(&path)
            .map_err(|err| format!("failed to read file '{}': {err}", path.display()))?;
        let rendered = String::from_utf8_lossy(&content[..content.len().min(MAX_READ_FILE_BYTES)])
            .into_owned();

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent.tool",
            format!(
                "read_file completed (id={}, path={}, bytes={})",
                request.id,
                path.display(),
                rendered.len()
            ),
        );

        Ok(rendered)
    }

    fn write_file(&self, request: &FlowLocalAgentRequest) -> Result<String, String> {
        let path = request
            .args
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "missing required string field 'path'".to_string())?;
        let content = request
            .args
            .get("content")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "missing required string field 'content'".to_string())?;
        let path = self.path_resolver.resolve(path)?;

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent.tool",
            format!(
                "write_file request (id={}, path={}, bytes={})",
                request.id,
                path.display(),
                content.len()
            ),
        );

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to create parent directory '{}': {err}",
                    parent.display()
                )
            })?;
        }

        fs::write(&path, content)
            .map_err(|err| format!("failed to write file '{}': {err}", path.display()))?;

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent.tool",
            format!(
                "write_file completed (id={}, path={}, bytes={})",
                request.id,
                path.display(),
                content.len()
            ),
        );

        Ok(format!(
            "written {} bytes to {}",
            content.len(),
            path.display()
        ))
    }

    async fn run_command(&self, request: &FlowLocalAgentRequest) -> Result<String, String> {
        let command = request
            .args
            .get("command")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "missing required string field 'command'".to_string())?;
        let cwd = request
            .args
            .get("cwd")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(|value| self.path_resolver.resolve(value))
            .transpose()?
            .unwrap_or_else(|| self.path_resolver.default_working_dir());
        let timeout_ms = request
            .args
            .get("timeout")
            .or_else(|| request.args.get("timeout_ms"))
            .and_then(value_as_u64_or_numeric_string)
            .unwrap_or(self.command_timeout_ms)
            .max(10);

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent.tool",
            format!(
                "run_command request (id={}, cwd={}, timeout_ms={}, command={})",
                request.id,
                cwd.display(),
                timeout_ms,
                command
            ),
        );

        let mut process = build_shell_command(command);
        process
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let child = process.spawn().map_err(|err| {
            format!(
                "failed to spawn command '{}' in '{}': {err}",
                command,
                cwd.display()
            )
        })?;

        let output = timeout(Duration::from_millis(timeout_ms), child.wait_with_output())
            .await
            .map_err(|_| {
                format!(
                    "command timed out after {} ms (cwd={}, command={})",
                    timeout_ms,
                    cwd.display(),
                    command
                )
            })?
            .map_err(|err| format!("failed to wait for command '{}': {err}", command))?;

        let mut merged = String::new();
        merged.push_str(String::from_utf8_lossy(&output.stdout).as_ref());
        merged.push_str(String::from_utf8_lossy(&output.stderr).as_ref());

        let truncated = truncate_to_char_boundary(merged, MAX_COMMAND_OUTPUT_BYTES);
        let status_text = output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated-by-signal".to_string());

        emit_console_log(
            LogLevel::Info,
            "flow.local_agent.tool",
            format!(
                "run_command completed (id={}, cwd={}, exit_status={}, output_bytes={}, command={})",
                request.id,
                cwd.display(),
                status_text,
                truncated.len(),
                command
            ),
        );

        Ok(truncated)
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

    fn default_working_dir(&self) -> PathBuf {
        self.base_dir.clone()
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

fn build_shell_command(command: &str) -> Command {
    if cfg!(windows) {
        let mut process = Command::new("cmd");
        process.args(["/C", command]);
        process
    } else {
        let mut process = Command::new("sh");
        process.args(["-lc", command]);
        process
    }
}

fn truncate_to_char_boundary(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value.truncate(end);
    value
}

fn value_as_u64_or_numeric_string(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_str()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn executor_lists_entries_with_upstream_shape() {
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
            .await
            .expect("list_files should succeed");

        let entries = serde_json::from_str::<serde_json::Value>(&output)
            .expect("output should be JSON array");
        let items = entries.as_array().expect("entries should be an array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "plan.md");
        assert_eq!(items[0]["type"], "file");

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[tokio::test]
    async fn executor_reads_absolute_file_path() {
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
            .await
            .expect("read_file should succeed");

        assert_eq!(output, "hello flow\n");

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[tokio::test]
    async fn executor_expands_home_directory_for_list_files() {
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
            .await
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

    #[tokio::test]
    async fn executor_rejects_disallowed_tool() {
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
            .await
            .expect_err("list_files should be rejected when not allowed");

        assert!(error.contains("not allowed"));

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[tokio::test]
    async fn executor_writes_file_and_reports_result() {
        let workspace_root = temp_path("flow-write");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");
        let target = workspace_root.join("notes").join("plan.md");

        let executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config(
            Some(workspace_root.clone()),
            vec!["write_file".to_string()],
        ));
        let output = executor
            .execute(&FlowLocalAgentRequest {
                id: "req-5".to_string(),
                tool: "write_file".to_string(),
                args: json!({"path": "notes/plan.md", "content": "hello write\n"}),
            })
            .await
            .expect("write_file should succeed");

        assert!(output.contains("written 12 bytes"));
        assert_eq!(fs::read_to_string(&target).expect("file should exist"), "hello write\n");

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[tokio::test]
    async fn executor_runs_command_from_workspace_root() {
        let workspace_root = temp_path("flow-run-command");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");
        let script = workspace_root.join("hello.txt");
        fs::write(&script, "from command\n").expect("test file should be written");

        let command = if cfg!(windows) {
            "type hello.txt"
        } else {
            "cat hello.txt"
        };

        let executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config(
            Some(workspace_root.clone()),
            vec!["run_command".to_string()],
        ));
        let output = executor
            .execute(&FlowLocalAgentRequest {
                id: "req-6".to_string(),
                tool: "run_command".to_string(),
                args: json!({"command": command}),
            })
            .await
            .expect("run_command should succeed");

        assert!(output.contains("from command"));

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[tokio::test]
    async fn executor_reports_command_timeout() {
        let workspace_root = temp_path("flow-command-timeout");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");

        let command = if cfg!(windows) {
            "ping 127.0.0.1 -n 6 > nul"
        } else {
            "sleep 5"
        };

        let executor = FlowLocalAgentToolExecutor::from_runtime_config(&runtime_config(
            Some(workspace_root.clone()),
            vec!["run_command".to_string()],
        ));
        let error = executor
            .execute(&FlowLocalAgentRequest {
                id: "req-7".to_string(),
                tool: "run_command".to_string(),
                args: json!({"command": command, "timeout": 100}),
            })
            .await
            .expect_err("run_command should time out");

        assert!(error.contains("timed out"));

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
