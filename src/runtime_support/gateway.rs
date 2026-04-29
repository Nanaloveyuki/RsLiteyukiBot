use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::onebot_support::value_to_string;

static LLM_API_KEY_ROUND_ROBIN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default)]
pub(crate) struct ExternalGatewaySnapshot {
    pub(crate) command_hits: u64,
    pub(crate) api_requests: u64,
    pub(crate) api_success: u64,
    pub(crate) api_failed: u64,
    pub(crate) api_timeouts: u64,
    pub(crate) api_inflight: usize,
}

#[derive(Debug)]
struct PendingApiCall {
    started_at: Instant,
}

#[derive(Debug, Default)]
struct ExternalGatewayState {
    command_hits: u64,
    api_requests: u64,
    api_success: u64,
    api_failed: u64,
    api_timeouts: u64,
    pending: HashMap<String, PendingApiCall>,
}

impl ExternalGatewayState {
    fn snapshot(&self) -> ExternalGatewaySnapshot {
        ExternalGatewaySnapshot {
            command_hits: self.command_hits,
            api_requests: self.api_requests,
            api_success: self.api_success,
            api_failed: self.api_failed,
            api_timeouts: self.api_timeouts,
            api_inflight: self.pending.len(),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ExternalGateway {
    state: Arc<Mutex<ExternalGatewayState>>,
    echo_seq: Arc<AtomicU64>,
}

impl ExternalGateway {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn snapshot(&self) -> ExternalGatewaySnapshot {
        self.state
            .lock()
            .expect("external gateway lock should not be poisoned")
            .snapshot()
    }

    pub(crate) fn next_echo(&self, prefix: &str) -> String {
        let seq = self.echo_seq.fetch_add(1, Ordering::SeqCst);
        format!("{prefix}-{seq}")
    }

    pub(crate) fn record_command_hit(&self) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        state.command_hits = state.command_hits.saturating_add(1);
        state.snapshot()
    }

    pub(crate) fn track_request(&self, echo: String) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        state.api_requests = state.api_requests.saturating_add(1);
        state.pending.insert(
            echo,
            PendingApiCall {
                started_at: Instant::now(),
            },
        );
        state.snapshot()
    }

    pub(crate) fn mark_send_failed(&self, echo: &str) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        if state.pending.remove(echo).is_some() {
            state.api_failed = state.api_failed.saturating_add(1);
        }
        state.snapshot()
    }

    pub(crate) fn observe_payload(
        &self,
        payload: &Value,
        timeout: Duration,
    ) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        sweep_pending_timeouts(&mut state, timeout);

        if let Some((echo, success)) = parse_onebot_v11_api_response(payload)
            && state.pending.remove(&echo).is_some()
        {
            if success {
                state.api_success = state.api_success.saturating_add(1);
            } else {
                state.api_failed = state.api_failed.saturating_add(1);
            }
        }

        state.snapshot()
    }

    pub(crate) fn sweep_timeouts(&self, timeout: Duration) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        sweep_pending_timeouts(&mut state, timeout);
        state.snapshot()
    }
}

fn sweep_pending_timeouts(state: &mut ExternalGatewayState, timeout: Duration) {
    let expired: Vec<String> = state
        .pending
        .iter()
        .filter_map(|(echo, call)| {
            if call.started_at.elapsed() >= timeout {
                Some(echo.clone())
            } else {
                None
            }
        })
        .collect();

    if expired.is_empty() {
        return;
    }

    for echo in expired {
        if state.pending.remove(&echo).is_some() {
            state.api_timeouts = state.api_timeouts.saturating_add(1);
        }
    }
}

fn parse_onebot_v11_api_response(payload: &Value) -> Option<(String, bool)> {
    let object = payload.as_object()?;
    if !object.contains_key("status") && !object.contains_key("retcode") {
        return None;
    }

    let echo = object.get("echo").and_then(value_to_string)?;
    let success = object
        .get("status")
        .and_then(Value::as_str)
        .map(|status| status.eq_ignore_ascii_case("ok"))
        .or_else(|| {
            object
                .get("retcode")
                .and_then(Value::as_i64)
                .map(|code| code == 0)
        })
        .unwrap_or(false);
    Some((echo, success))
}

pub(crate) fn next_llm_api_key_index(key_count: usize) -> Option<usize> {
    if key_count == 0 {
        return None;
    }
    Some(LLM_API_KEY_ROUND_ROBIN.fetch_add(1, Ordering::SeqCst) as usize % key_count)
}
