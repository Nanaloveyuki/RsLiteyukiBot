use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowLocalAgentRuntimeSnapshot {
    pub connected: bool,
    pub reconnect_allowed: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Default)]
struct FlowLocalAgentStateInner {
    connected: bool,
    reconnect_allowed: bool,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FlowLocalAgentRuntimeState {
    inner: Arc<Mutex<FlowLocalAgentStateInner>>,
}

impl FlowLocalAgentRuntimeState {
    pub fn snapshot(&self) -> FlowLocalAgentRuntimeSnapshot {
        let inner = self
            .inner
            .lock()
            .expect("flow local agent state lock should not be poisoned");
        FlowLocalAgentRuntimeSnapshot {
            connected: inner.connected,
            reconnect_allowed: inner.reconnect_allowed,
            last_error: inner.last_error.clone(),
        }
    }

    pub fn mark_connected(&self) {
        let mut inner = self
            .inner
            .lock()
            .expect("flow local agent state lock should not be poisoned");
        inner.connected = true;
        inner.reconnect_allowed = true;
        inner.last_error = None;
    }

    pub fn mark_disconnected(&self, reconnect_allowed: bool, error: Option<String>) {
        let mut inner = self
            .inner
            .lock()
            .expect("flow local agent state lock should not be poisoned");
        inner.connected = false;
        inner.reconnect_allowed = reconnect_allowed;
        inner.last_error = error;
    }
}
