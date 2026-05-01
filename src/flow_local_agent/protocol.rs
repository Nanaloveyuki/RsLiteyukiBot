use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowLocalAgentCloseCode {
    InvalidToken = 4001,
    ReplacedBySameDevice = 4002,
    RevokedOrRemoved = 4003,
}

impl FlowLocalAgentCloseCode {
    pub fn should_reconnect(code: u16) -> bool {
        !matches!(
            code,
            x if x == Self::InvalidToken as u16
                || x == Self::ReplacedBySameDevice as u16
                || x == Self::RevokedOrRemoved as u16
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowLocalAgentRequest {
    pub id: String,
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowLocalAgentToolResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowLocalAgentApprovalResponse {
    pub id: String,
    pub approved: bool,
    #[serde(default)]
    pub always: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FlowLocalAgentClientMessage {
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "confirm_request")]
    ConfirmRequest {
        id: String,
        tool: String,
        #[serde(default)]
        args: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FlowLocalAgentServerMessage {
    ConfirmResponse(FlowLocalAgentConfirmEnvelope),
    Ping(FlowLocalAgentPingMessage),
    Request(FlowLocalAgentRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowLocalAgentPingMessage {
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowLocalAgentConfirmEnvelope {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub approved: bool,
    #[serde(default)]
    pub always: bool,
}
