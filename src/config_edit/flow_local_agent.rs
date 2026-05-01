use crate::utils::llm_config::{
    normalize_lowercase_non_empty_string, normalize_non_empty_string,
    normalize_provider_url, normalize_string_entries_preserve_order,
};

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // 外部调用
pub struct FlowLocalAgentConfigPatch {
    pub enabled: Option<bool>,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub auto_connect: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
    pub workspace_root: Option<String>,
    pub command_timeout_seconds: Option<u64>,
    pub approval_policy: Option<String>,
}

#[allow(dead_code)]
pub(crate) fn describe_flow_local_agent_patch(patch: &FlowLocalAgentConfigPatch) -> String {
    let mut fields = Vec::new();
    if let Some(enabled) = patch.enabled {
        fields.push(format!("enabled={enabled}"));
    }
    if let Some(base_url) = patch.base_url.as_deref() {
        fields.push(format!("base_url={base_url}"));
    }
    if patch.token.is_some() {
        fields.push("token=<updated>".to_string());
    }
    if let Some(device_id) = patch.device_id.as_deref() {
        fields.push(format!("device_id={device_id}"));
    }
    if let Some(device_name) = patch.device_name.as_deref() {
        fields.push(format!("device_name={device_name}"));
    }
    if let Some(auto_connect) = patch.auto_connect {
        fields.push(format!("auto_connect={auto_connect}"));
    }
    if let Some(allowed_tools) = patch.allowed_tools.as_ref() {
        fields.push(format!("allowed_tools={}", allowed_tools.len()));
    }
    if let Some(workspace_root) = patch.workspace_root.as_deref() {
        fields.push(format!("workspace_root={workspace_root}"));
    }
    if let Some(command_timeout_seconds) = patch.command_timeout_seconds {
        fields.push(format!("command_timeout_seconds={command_timeout_seconds}"));
    }
    if let Some(approval_policy) = patch.approval_policy.as_deref() {
        fields.push(format!("approval_policy={approval_policy}"));
    }

    if fields.is_empty() {
        "fields=none".to_string()
    } else {
        fields.join(", ")
    }
}

#[allow(dead_code)]
pub(crate) fn normalize_flow_local_agent_patch(
    patch: &FlowLocalAgentConfigPatch,
) -> FlowLocalAgentConfigPatch {
    FlowLocalAgentConfigPatch {
        enabled: patch.enabled,
        base_url: patch
            .base_url
            .as_ref()
            .map(|value| normalize_provider_url(value.as_str()).unwrap_or_default()),
        token: patch
            .token
            .as_ref()
            .map(|value| normalize_non_empty_string(value.as_str()).unwrap_or_default()),
        device_id: patch
            .device_id
            .as_ref()
            .map(|value| normalize_non_empty_string(value.as_str()).unwrap_or_default()),
        device_name: patch
            .device_name
            .as_ref()
            .map(|value| normalize_non_empty_string(value.as_str()).unwrap_or_default()),
        auto_connect: patch.auto_connect,
        allowed_tools: patch.allowed_tools.as_ref().map(|tools| {
            let normalized = tools
                .iter()
                .filter_map(|tool| normalize_lowercase_non_empty_string(tool.as_str()))
                .collect::<Vec<_>>();
            normalize_string_entries_preserve_order(normalized.as_slice())
        }),
        workspace_root: patch
            .workspace_root
            .as_ref()
            .map(|value| normalize_non_empty_string(value.as_str()).unwrap_or_default()),
        command_timeout_seconds: patch.command_timeout_seconds.filter(|value| *value > 0),
        approval_policy: patch
            .approval_policy
            .as_ref()
            .map(|value| normalize_lowercase_non_empty_string(value.as_str()).unwrap_or_default()),
    }
}
