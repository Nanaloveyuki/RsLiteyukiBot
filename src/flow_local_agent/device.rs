use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::app_config::FlowLocalAgentRuntimeConfig;
use crate::utils::config_path::resolve_preferred_flow_local_agent_device_id_path;

pub(crate) fn normalize_runtime_config(
    mut config: FlowLocalAgentRuntimeConfig,
) -> (FlowLocalAgentRuntimeConfig, Vec<String>) {
    let mut warnings = Vec::new();

    let configured = config
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if configured.is_some() {
        config.device_id = configured;
        return (config, warnings);
    }

    let path = resolve_preferred_flow_local_agent_device_id_path();
    match load_or_create_device_id(path.as_path()) {
        Ok(device_id) => {
            config.device_id = Some(device_id);
        }
        Err(err) => {
            warnings.push(format!(
                "flow local agent device_id persistence unavailable: {err}"
            ));
        }
    }

    (config, warnings)
}

fn load_or_create_device_id(path: &Path) -> Result<String, String> {
    if let Some(existing) = read_device_id(path)? {
        return Ok(existing);
    }

    let generated = Uuid::new_v4().to_string();
    persist_device_id(path, generated.as_str())?;
    Ok(generated)
}

fn read_device_id(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(trimmed.to_string()))
}

fn persist_device_id(path: &Path, device_id: &str) -> Result<(), String> {
    ensure_parent_dir(path)?;
    fs::write(path, format!("{device_id}\n"))
        .map_err(|err| format!("failed to persist {}: {err}", path.display()))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create flow local agent device_id directory {}: {err}",
                parent.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalize_runtime_config_preserves_configured_device_id() {
        let config = runtime_config(Some("configured-device".to_string()));
        let (normalized, warnings) = normalize_runtime_config(config);

        assert_eq!(normalized.device_id.as_deref(), Some("configured-device"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn load_or_create_device_id_reuses_existing_file() {
        let path = temp_path("reuse-device-id");
        persist_device_id(path.as_path(), "persisted-device").expect("device id should persist");

        let device_id = load_or_create_device_id(path.as_path()).expect("device id should load");
        assert_eq!(device_id, "persisted-device");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_or_create_device_id_generates_and_persists_uuid() {
        let path = temp_path("generate-device-id");
        let _ = fs::remove_file(&path);

        let device_id =
            load_or_create_device_id(path.as_path()).expect("device id should generate");
        assert!(Uuid::parse_str(device_id.as_str()).is_ok());

        let persisted = fs::read_to_string(&path).expect("device id file should exist");
        assert_eq!(persisted.trim(), device_id);

        let _ = fs::remove_file(path);
    }

    fn runtime_config(device_id: Option<String>) -> FlowLocalAgentRuntimeConfig {
        FlowLocalAgentRuntimeConfig {
            enabled: true,
            base_url: Some("https://flow.liteyuki.org".to_string()),
            token: Some("lys_test".to_string()),
            device_id,
            device_name: Some("Test Device".to_string()),
            auto_connect: true,
            allowed_tools: vec!["read_file".to_string()],
            workspace_root: None,
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
            "liteyuki-flow-local-agent-device-test-{label}-{process_id}-{unique}"
        ))
    }
}
