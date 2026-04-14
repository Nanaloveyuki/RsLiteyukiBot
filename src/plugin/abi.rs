use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::PluginRuntimeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginAbiMethod {
    Initialize,
    Shutdown,
    HealthCheck,
    HandleEvent,
    CallHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginErrorCode {
    Ok,
    Unknown,
    HostUnavailable,
    InvalidRequest,
    InvalidArgument,
    NotImplemented,
    PermissionDenied,
    Timeout,
    DependencyMissing,
    RuntimeInitFailed,
    RuntimeCrashed,
    EntryNotFound,
    UnsupportedAbi,
    SerializationFailed,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAbiContract {
    pub runtime: PluginRuntimeKind,
    pub abi_name: String,
    pub abi_version: String,
    pub host_api_version: String,
    pub required_methods: Vec<PluginAbiMethod>,
    pub supported_error_codes: Vec<PluginErrorCode>,
}

impl PluginAbiContract {
    pub fn new(
        runtime: PluginRuntimeKind,
        abi_name: impl Into<String>,
        abi_version: impl Into<String>,
        host_api_version: impl Into<String>,
    ) -> Self {
        Self {
            runtime,
            abi_name: abi_name.into(),
            abi_version: abi_version.into(),
            host_api_version: host_api_version.into(),
            required_methods: vec![
                PluginAbiMethod::Initialize,
                PluginAbiMethod::Shutdown,
                PluginAbiMethod::HealthCheck,
                PluginAbiMethod::CallHost,
            ],
            supported_error_codes: vec![
                PluginErrorCode::Ok,
                PluginErrorCode::InvalidRequest,
                PluginErrorCode::InvalidArgument,
                PluginErrorCode::NotImplemented,
                PluginErrorCode::PermissionDenied,
                PluginErrorCode::Timeout,
                PluginErrorCode::DependencyMissing,
                PluginErrorCode::RuntimeInitFailed,
                PluginErrorCode::RuntimeCrashed,
                PluginErrorCode::EntryNotFound,
                PluginErrorCode::UnsupportedAbi,
                PluginErrorCode::SerializationFailed,
                PluginErrorCode::Internal,
                PluginErrorCode::Unknown,
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHandshakeRequest {
    pub plugin_id: String,
    pub runtime: PluginRuntimeKind,
    pub requested_abi_version: String,
    pub host_api_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHandshakeResponse {
    pub accepted: bool,
    pub contract: Option<PluginAbiContract>,
    pub code: PluginErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCallEnvelope {
    pub method: PluginAbiMethod,
    pub correlation_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCallResult {
    pub ok: bool,
    pub code: PluginErrorCode,
    pub message: String,
    pub payload: Option<Value>,
}

