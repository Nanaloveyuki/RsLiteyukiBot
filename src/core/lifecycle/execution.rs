#[path = "execution/registration.rs"]
mod registration;
#[path = "execution/runner.rs"]
mod runner;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::observability::Logger;

use super::context::{HookFilter, LifecycleContext};

pub(super) const MODULE_LIFECYCLE: &str = "core.lifecycle";
pub(super) const DEFAULT_PROCESS_NAME: &str = "main";

pub(super) type HookFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;
pub(super) type HookHandler =
    Arc<dyn Fn(Arc<LifecycleContext>) -> HookFuture + Send + Sync + 'static>;
pub(super) type ProcessHookHandler =
    Arc<dyn Fn(Arc<LifecycleContext>, Arc<str>) -> HookFuture + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePhase {
    BeforeStart,
    AfterStart,
    BeforeProcessShutdown,
    AfterShutdown,
    BeforeProcessRestart,
    AfterRestart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleFailurePolicy {
    FailFast,
    Continue,
}

#[derive(Debug, Clone)]
pub struct HookFailure {
    pub hook_name: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct LifecycleExecutionError {
    pub phase: LifecyclePhase,
    pub failures: Vec<HookFailure>,
}

impl std::fmt::Display for LifecycleExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lifecycle phase {:?} failed with {} error(s)",
            self.phase,
            self.failures.len()
        )
    }
}

impl std::error::Error for LifecycleExecutionError {}

#[derive(Clone)]
pub(super) struct HookRegistration {
    pub(super) name: String,
    pub(super) filter: HookFilter,
    pub(super) handler: HookHandler,
}

#[derive(Clone)]
pub(super) struct ProcessHookRegistration {
    pub(super) name: String,
    pub(super) filter: HookFilter,
    pub(super) handler: ProcessHookHandler,
}

#[derive(Clone)]
pub struct Lifespan {
    pub(super) before_start_hooks: Vec<HookRegistration>,
    pub(super) after_start_hooks: Vec<HookRegistration>,
    pub(super) before_process_shutdown_hooks: Vec<ProcessHookRegistration>,
    pub(super) after_shutdown_hooks: Vec<HookRegistration>,
    pub(super) before_process_restart_hooks: Vec<ProcessHookRegistration>,
    pub(super) after_restart_hooks: Vec<HookRegistration>,
    pub(super) failure_policy: LifecycleFailurePolicy,
    pub(super) hook_timeout: Option<Duration>,
    pub(super) logger: Option<Logger>,
}

impl Default for Lifespan {
    fn default() -> Self {
        Self {
            before_start_hooks: Vec::new(),
            after_start_hooks: Vec::new(),
            before_process_shutdown_hooks: Vec::new(),
            after_shutdown_hooks: Vec::new(),
            before_process_restart_hooks: Vec::new(),
            after_restart_hooks: Vec::new(),
            failure_policy: LifecycleFailurePolicy::FailFast,
            hook_timeout: None,
            logger: None,
        }
    }
}
