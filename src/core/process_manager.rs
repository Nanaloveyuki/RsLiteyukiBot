use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{sleep, timeout};

use crate::observability::Logger;

const MODULE_PROCESS_MANAGER: &str = "core.process_manager";
const SUPERVISOR_COMMAND_QUEUE_CAPACITY: usize = 16;
const SUPERVISOR_COMMAND_SEND_TIMEOUT: Duration = Duration::from_secs(2);
const SUPERVISOR_JOIN_TIMEOUT: Duration = Duration::from_secs(5);
const SUPERVISOR_RESTART_TIMEOUT: Duration = Duration::from_secs(5);

type RunnerFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;
pub type ManagedProcessRunner =
    Arc<dyn Fn(watch::Receiver<bool>) -> RunnerFuture + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

#[derive(Debug, Clone)]
pub struct ManagedProcessSpec {
    pub name: Arc<str>,
    pub restart_policy: RestartPolicy,
    pub shutdown_timeout: Duration,
}

impl ManagedProcessSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Arc::<str>::from(name.into()),
            restart_policy: RestartPolicy::Never,
            shutdown_timeout: Duration::from_secs(3),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProcessManagerError {
    AlreadyRegistered(String),
    NotFound(String),
    AlreadyRunning(String),
    NotRunning(String),
    SupervisorChannelClosed(String),
    SupervisorCommandTimeout(String),
    SupervisorJoinTimeout(String),
    SupervisorJoinFailed(String),
    RestartTimeout(String),
}

impl std::fmt::Display for ProcessManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered(name) => write!(f, "process '{}' already registered", name),
            Self::NotFound(name) => write!(f, "process '{}' not found", name),
            Self::AlreadyRunning(name) => write!(f, "process '{}' already running", name),
            Self::NotRunning(name) => write!(f, "process '{}' not running", name),
            Self::SupervisorChannelClosed(name) => {
                write!(f, "process '{}' supervisor channel closed", name)
            }
            Self::SupervisorCommandTimeout(name) => {
                write!(f, "process '{}' supervisor command send timeout", name)
            }
            Self::SupervisorJoinTimeout(name) => {
                write!(f, "process '{}' supervisor join timeout", name)
            }
            Self::SupervisorJoinFailed(name) => {
                write!(f, "process '{}' supervisor join failed", name)
            }
            Self::RestartTimeout(name) => {
                write!(f, "process '{}' restart timed out", name)
            }
        }
    }
}

impl std::error::Error for ProcessManagerError {}

#[derive(Clone)]
struct Registration {
    spec: ManagedProcessSpec,
    runner: ManagedProcessRunner,
}

struct RunningProcess {
    command_tx: mpsc::Sender<SupervisorCommand>,
    restart_generation_rx: watch::Receiver<u64>,
    supervisor: JoinHandle<()>,
}

#[derive(Debug, Clone, Copy)]
enum SupervisorCommand {
    Restart,
    Shutdown,
}

#[derive(Clone, Default)]
pub struct ProcessManager {
    registrations: Arc<RwLock<HashMap<Arc<str>, Registration>>>,
    running: Arc<Mutex<HashMap<Arc<str>, RunningProcess>>>,
    logger: Option<Logger>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_logger(logger: Logger) -> Self {
        Self {
            logger: Some(logger),
            ..Self::default()
        }
    }

    pub fn set_logger(&mut self, logger: Logger) {
        self.logger = Some(logger);
    }

    pub fn register<F, Fut>(
        &self,
        name: impl Into<String>,
        mut spec: ManagedProcessSpec,
        runner: F,
    ) -> Result<(), ProcessManagerError>
    where
        F: Fn(watch::Receiver<bool>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let name: Arc<str> = Arc::from(name.into());
        spec.name = Arc::clone(&name);
        let runner: ManagedProcessRunner =
            Arc::new(move |shutdown_rx| Box::pin(runner(shutdown_rx)));

        let mut registrations = self
            .registrations
            .write()
            .expect("process registrations lock should not be poisoned");
        if registrations.contains_key(&name) {
            return Err(ProcessManagerError::AlreadyRegistered(name.to_string()));
        }
        registrations.insert(name, Registration { spec, runner });
        Ok(())
    }

    pub fn start(&self, name: &str) -> Result<(), ProcessManagerError> {
        let name_key: Arc<str> = Arc::from(name.to_string());
        let registration = self
            .registrations
            .read()
            .expect("process registrations lock should not be poisoned")
            .get(&name_key)
            .cloned()
            .ok_or_else(|| ProcessManagerError::NotFound(name.to_string()))?;

        let mut running = self
            .running
            .lock()
            .expect("process running lock should not be poisoned");
        prune_finished(&mut running);
        if running.contains_key(&name_key) {
            return Err(ProcessManagerError::AlreadyRunning(name.to_string()));
        }

        let (command_tx, command_rx) = mpsc::channel(SUPERVISOR_COMMAND_QUEUE_CAPACITY);
        let (restart_generation_tx, restart_generation_rx) = watch::channel(0u64);
        let logger = self.logger.clone();
        let supervisor_name = Arc::clone(&registration.spec.name);
        let supervisor = tokio::spawn(async move {
            supervise_process(registration, command_rx, restart_generation_tx, logger).await;
        });

        if let Some(logger) = &self.logger {
            logger.debug_in(
                MODULE_PROCESS_MANAGER,
                format!("process '{}' supervisor started", supervisor_name),
            );
        }

        running.insert(
            name_key,
            RunningProcess {
                command_tx,
                restart_generation_rx,
                supervisor,
            },
        );
        Ok(())
    }

    pub fn start_all(&self) -> Result<(), ProcessManagerError> {
        let names: Vec<String> = self
            .registrations
            .read()
            .expect("process registrations lock should not be poisoned")
            .keys()
            .map(ToString::to_string)
            .collect();

        for name in names {
            if let Err(err) = self.start(&name)
                && !matches!(err, ProcessManagerError::AlreadyRunning(_))
            {
                return Err(err);
            }
        }
        Ok(())
    }

    pub async fn restart(&self, name: &str) -> Result<(), ProcessManagerError> {
        let (command_tx, mut restart_generation_rx, current_generation) = {
            let mut running = self
                .running
                .lock()
                .expect("process running lock should not be poisoned");
            prune_finished(&mut running);

            let process = running
                .get(name)
                .ok_or_else(|| ProcessManagerError::NotRunning(name.to_string()))?;
            (
                process.command_tx.clone(),
                process.restart_generation_rx.clone(),
                *process.restart_generation_rx.borrow(),
            )
        };

        match command_tx.try_send(SupervisorCommand::Restart) {
            Ok(_) => Ok(()),
            Err(TrySendError::Full(_)) => {
                if let Some(logger) = &self.logger {
                    logger.debug_in(
                        MODULE_PROCESS_MANAGER,
                        format!(
                            "process '{}' restart already queued; dropping duplicate command",
                            name
                        ),
                    );
                }
                Ok(())
            }
            Err(TrySendError::Closed(_)) => Err(ProcessManagerError::SupervisorChannelClosed(
                name.to_string(),
            )),
        }?;

        timeout(SUPERVISOR_RESTART_TIMEOUT, async {
            loop {
                if *restart_generation_rx.borrow() != current_generation {
                    return Ok(());
                }
                restart_generation_rx
                    .changed()
                    .await
                    .map_err(|_| ProcessManagerError::SupervisorChannelClosed(name.to_string()))?;
            }
        })
        .await
        .map_err(|_| ProcessManagerError::RestartTimeout(name.to_string()))?
    }

    pub async fn terminate(&self, name: &str) -> Result<(), ProcessManagerError> {
        let process = {
            let mut running = self
                .running
                .lock()
                .expect("process running lock should not be poisoned");
            prune_finished(&mut running);
            running
                .remove(name)
                .ok_or_else(|| ProcessManagerError::NotRunning(name.to_string()))?
        };

        terminate_running_process(name.to_string(), process, self.logger.clone()).await
    }

    pub async fn terminate_all(&self) -> Result<(), ProcessManagerError> {
        let processes: Vec<(String, RunningProcess)> = {
            let mut running = self
                .running
                .lock()
                .expect("process running lock should not be poisoned");
            prune_finished(&mut running);
            running
                .drain()
                .map(|(name, process)| (name.to_string(), process))
                .collect()
        };

        let mut join_set = JoinSet::new();
        for (name, process) in processes {
            let logger = self.logger.clone();
            join_set.spawn(async move { terminate_running_process(name, process, logger).await });
        }

        let mut first_error: Option<ProcessManagerError> = None;
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
                Err(err) => {
                    if first_error.is_none() {
                        first_error =
                            Some(ProcessManagerError::SupervisorJoinFailed(err.to_string()));
                    }
                }
            }
        }

        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    pub fn is_running(&self, name: &str) -> bool {
        let mut running = self
            .running
            .lock()
            .expect("process running lock should not be poisoned");
        prune_finished(&mut running);
        running.contains_key(name)
    }

    pub fn running_process_names(&self) -> Vec<Arc<str>> {
        let mut running = self
            .running
            .lock()
            .expect("process running lock should not be poisoned");
        prune_finished(&mut running);
        running.keys().cloned().collect()
    }
}

fn prune_finished(running: &mut HashMap<Arc<str>, RunningProcess>) {
    running.retain(|_, process| !process.supervisor.is_finished());
}

async fn supervise_process(
    registration: Registration,
    mut command_rx: mpsc::Receiver<SupervisorCommand>,
    restart_generation_tx: watch::Sender<u64>,
    logger: Option<Logger>,
) {
    let name = Arc::clone(&registration.spec.name);

    loop {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let runner = Arc::clone(&registration.runner);
        let mut worker = tokio::spawn((runner)(shutdown_rx));
        let next_generation = {
            let current_generation = *restart_generation_tx.borrow();
            current_generation + 1
        };
        let _ = restart_generation_tx.send(next_generation);

        if let Some(logger) = &logger {
            logger.info_in(
                MODULE_PROCESS_MANAGER,
                format!("process '{}' started", name),
            );
        }

        let pending_restart = tokio::select! {
            command = command_rx.recv() => {
                match command {
                    Some(SupervisorCommand::Restart) => {
                        if let Some(logger) = &logger {
                            logger.warn_in(MODULE_PROCESS_MANAGER, format!("process '{}' restarting by command", name));
                        }
                        let _ = shutdown_tx.send(true);
                        shutdown_worker(&mut worker, registration.spec.shutdown_timeout, &logger, name.as_ref()).await;
                        true
                    }
                    Some(SupervisorCommand::Shutdown) | None => {
                        if let Some(logger) = &logger {
                            logger.info_in(MODULE_PROCESS_MANAGER, format!("process '{}' shutdown requested", name));
                        }
                        let _ = shutdown_tx.send(true);
                        shutdown_worker(&mut worker, registration.spec.shutdown_timeout, &logger, name.as_ref()).await;
                        return;
                    }
                }
            }
            worker_result = &mut worker => {
                let failed = match worker_result {
                    Ok(Ok(())) => false,
                    Ok(Err(reason)) => {
                        if let Some(logger) = &logger {
                            logger.warn_in(MODULE_PROCESS_MANAGER, format!("process '{}' failed: {}", name, reason));
                        }
                        true
                    }
                    Err(err) => {
                        if let Some(logger) = &logger {
                            logger.warn_in(MODULE_PROCESS_MANAGER, format!("process '{}' join error: {}", name, err));
                        }
                        true
                    }
                };

                match registration.spec.restart_policy {
                    RestartPolicy::Never => false,
                    RestartPolicy::OnFailure => failed,
                    RestartPolicy::Always => true,
                }
            }
        };

        if !pending_restart {
            if let Some(logger) = &logger {
                logger.info_in(
                    MODULE_PROCESS_MANAGER,
                    format!("process '{}' stopped", name),
                );
            }
            return;
        }

        sleep(Duration::from_millis(50)).await;
    }
}

async fn shutdown_worker(
    worker: &mut JoinHandle<Result<(), String>>,
    timeout_duration: Duration,
    logger: &Option<Logger>,
    name: &str,
) {
    match timeout(timeout_duration, &mut *worker).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(reason))) => {
            if let Some(logger) = logger {
                logger.warn_in(
                    MODULE_PROCESS_MANAGER,
                    format!("process '{}' stopped with error: {}", name, reason),
                );
            }
        }
        Ok(Err(join_err)) => {
            if let Some(logger) = logger {
                logger.warn_in(
                    MODULE_PROCESS_MANAGER,
                    format!("process '{}' join error while stopping: {}", name, join_err),
                );
            }
        }
        Err(_) => {
            if let Some(logger) = logger {
                logger.warn_in(
                    MODULE_PROCESS_MANAGER,
                    format!("process '{}' shutdown timeout, aborting worker", name),
                );
            }
            worker.abort();
            let _ = worker.await;
        }
    }
}

async fn terminate_running_process(
    name: String,
    process: RunningProcess,
    logger: Option<Logger>,
) -> Result<(), ProcessManagerError> {
    match process.command_tx.try_send(SupervisorCommand::Shutdown) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            if let Some(logger) = &logger {
                logger.warn_in(
                    MODULE_PROCESS_MANAGER,
                    format!(
                        "process '{}' shutdown queue full, waiting for supervisor",
                        name
                    ),
                );
            }
            let send_result = timeout(
                SUPERVISOR_COMMAND_SEND_TIMEOUT,
                process.command_tx.send(SupervisorCommand::Shutdown),
            )
            .await
            .map_err(|_| ProcessManagerError::SupervisorCommandTimeout(name.clone()))?;
            send_result.map_err(|_| ProcessManagerError::SupervisorChannelClosed(name.clone()))?;
        }
        Err(TrySendError::Closed(_)) => {
            return Err(ProcessManagerError::SupervisorChannelClosed(name));
        }
    }

    let mut supervisor = process.supervisor;
    match timeout(SUPERVISOR_JOIN_TIMEOUT, &mut supervisor).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(ProcessManagerError::SupervisorJoinFailed(format!(
            "{} ({})",
            name, err
        ))),
        Err(_) => {
            if let Some(logger) = &logger {
                logger.warn_in(
                    MODULE_PROCESS_MANAGER,
                    format!("process '{}' supervisor join timeout, aborting", name),
                );
            }
            supervisor.abort();
            let _ = supervisor.await;
            Err(ProcessManagerError::SupervisorJoinTimeout(name))
        }
    }
}
