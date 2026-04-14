use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

use crate::observability::Logger;

const MODULE_PROCESS_MANAGER: &str = "core.process_manager";

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
    command_tx: mpsc::UnboundedSender<SupervisorCommand>,
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
        let mut manager = Self::default();
        manager.logger = Some(logger);
        manager
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
        let runner: ManagedProcessRunner = Arc::new(move |shutdown_rx| Box::pin(runner(shutdown_rx)));

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

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let logger = self.logger.clone();
        let supervisor_name = Arc::clone(&registration.spec.name);
        let supervisor = tokio::spawn(async move {
            supervise_process(registration, command_rx, logger).await;
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
            if let Err(err) = self.start(&name) {
                if !matches!(err, ProcessManagerError::AlreadyRunning(_)) {
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    pub async fn restart(&self, name: &str) -> Result<(), ProcessManagerError> {
        let mut running = self
            .running
            .lock()
            .expect("process running lock should not be poisoned");
        prune_finished(&mut running);

        let process = running
            .get(name)
            .ok_or_else(|| ProcessManagerError::NotRunning(name.to_string()))?;
        process
            .command_tx
            .send(SupervisorCommand::Restart)
            .map_err(|_| ProcessManagerError::SupervisorChannelClosed(name.to_string()))
    }

    pub async fn terminate(&self, name: &str) -> Result<(), ProcessManagerError> {
        let mut running = self
            .running
            .lock()
            .expect("process running lock should not be poisoned");
        prune_finished(&mut running);
        let process = running
            .remove(name)
            .ok_or_else(|| ProcessManagerError::NotRunning(name.to_string()))?;
        drop(running);

        process
            .command_tx
            .send(SupervisorCommand::Shutdown)
            .map_err(|_| ProcessManagerError::SupervisorChannelClosed(name.to_string()))?;

        let _ = process.supervisor.await;
        Ok(())
    }

    pub async fn terminate_all(&self) -> Result<(), ProcessManagerError> {
        let names: Vec<String> = {
            let mut running = self
                .running
                .lock()
                .expect("process running lock should not be poisoned");
            prune_finished(&mut running);
            running.keys().map(ToString::to_string).collect()
        };

        for name in names {
            self.terminate(&name).await?;
        }
        Ok(())
    }

    pub fn is_running(&self, name: &str) -> bool {
        let mut running = self
            .running
            .lock()
            .expect("process running lock should not be poisoned");
        prune_finished(&mut running);
        running.contains_key(name)
    }
}

fn prune_finished(running: &mut HashMap<Arc<str>, RunningProcess>) {
    running.retain(|_, process| !process.supervisor.is_finished());
}

async fn supervise_process(
    registration: Registration,
    mut command_rx: mpsc::UnboundedReceiver<SupervisorCommand>,
    logger: Option<Logger>,
) {
    let name = Arc::clone(&registration.spec.name);

    loop {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let runner = Arc::clone(&registration.runner);
        let mut worker = tokio::spawn((runner)(shutdown_rx));

        if let Some(logger) = &logger {
            logger.info_in(
                MODULE_PROCESS_MANAGER,
                format!("process '{}' started", name),
            );
        }

        let pending_restart;
        loop {
            tokio::select! {
                command = command_rx.recv() => {
                    match command {
                        Some(SupervisorCommand::Restart) => {
                            pending_restart = true;
                            if let Some(logger) = &logger {
                                logger.warn_in(MODULE_PROCESS_MANAGER, format!("process '{}' restarting by command", name));
                            }
                            let _ = shutdown_tx.send(true);
                            shutdown_worker(&mut worker, registration.spec.shutdown_timeout, &logger, name.as_ref()).await;
                            break;
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

                    pending_restart = match registration.spec.restart_policy {
                        RestartPolicy::Never => false,
                        RestartPolicy::OnFailure => failed,
                        RestartPolicy::Always => true,
                    };

                    break;
                }
            }
        }

        if !pending_restart {
            if let Some(logger) = &logger {
                logger.info_in(MODULE_PROCESS_MANAGER, format!("process '{}' stopped", name));
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
