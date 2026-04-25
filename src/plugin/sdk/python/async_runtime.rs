use std::sync::OnceLock;
use std::sync::mpsc;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyIterator, PyTuple};

struct PythonAwaitRequest {
    awaitable: Py<PyAny>,
    response_tx: mpsc::Sender<Result<Py<PyAny>, String>>,
}

struct PythonAsyncRuntime {
    request_tx: mpsc::Sender<PythonAwaitRequest>,
}

impl PythonAsyncRuntime {
    fn shared() -> &'static Self {
        static RUNTIME: OnceLock<PythonAsyncRuntime> = OnceLock::new();
        RUNTIME.get_or_init(Self::spawn)
    }

    fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<PythonAwaitRequest>();
        std::thread::Builder::new()
            .name("liteyuki-python-async".to_string())
            .spawn(move || {
                let loop_ref = Python::with_gil(|py| -> PyResult<Py<PyAny>> {
                    let asyncio = py.import("asyncio")?;
                    let loop_ref = asyncio.call_method0("new_event_loop")?;
                    asyncio.call_method1("set_event_loop", (loop_ref.clone(),))?;
                    Ok(loop_ref.unbind())
                })
                .expect("python async runtime loop should initialize");

                while let Ok(request) = request_rx.recv() {
                    let result = Python::with_gil(|py| {
                        run_awaitable_in_loop(py, loop_ref.bind(py), request.awaitable)
                    });
                    let outbound = match result {
                        Ok(value) => Ok(value),
                        Err(err) => Err(err.to_string()),
                    };
                    let _ = request.response_tx.send(outbound);
                }

                Python::with_gil(|py| {
                    let loop_ref = loop_ref.bind(py);
                    let _ = loop_ref.call_method0("close");
                });
            })
            .expect("python async runtime thread should spawn");
        Self { request_tx }
    }

    fn await_result(&self, py: Python<'_>, awaitable: Py<PyAny>) -> PyResult<Py<PyAny>> {
        let (response_tx, response_rx) = mpsc::channel();
        let request_tx = self.request_tx.clone();
        let response = py
            .allow_threads(move || {
                request_tx
                    .send(PythonAwaitRequest {
                        awaitable,
                        response_tx,
                    })
                    .map_err(|_| "python async runtime thread is unavailable".to_string())?;
                response_rx
                    .recv()
                    .map_err(|_| "python async runtime response channel closed".to_string())
            })
            .map_err(PyRuntimeError::new_err)?;
        response.map_err(PyRuntimeError::new_err)
    }
}

pub(super) fn await_python_awaitable(py: Python<'_>, awaitable: Py<PyAny>) -> PyResult<Py<PyAny>> {
    PythonAsyncRuntime::shared().await_result(py, awaitable)
}

fn run_awaitable_in_loop(
    py: Python<'_>,
    loop_ref: &pyo3::Bound<'_, PyAny>,
    awaitable: Py<PyAny>,
) -> PyResult<Py<PyAny>> {
    let asyncio = py.import("asyncio")?;
    asyncio.call_method1("set_event_loop", (loop_ref,))?;

    let result = loop_ref.call_method1("run_until_complete", (awaitable.bind(py),));
    let cleanup_result = cancel_pending_tasks(py, loop_ref);

    match (result, cleanup_result) {
        (Ok(value), Ok(())) => Ok(value.unbind()),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(err)) => Err(err),
        (Err(err), Err(_cleanup_err)) => Err(err),
    }
}

fn cancel_pending_tasks(py: Python<'_>, loop_ref: &pyo3::Bound<'_, PyAny>) -> PyResult<()> {
    let asyncio = py.import("asyncio")?;
    let pending = asyncio.call_method1("all_tasks", (loop_ref,))?;
    let pending_iter = PyIterator::from_object(&pending)?;
    let mut tasks = Vec::new();
    for item in pending_iter {
        tasks.push(item?.unbind());
    }
    if tasks.is_empty() {
        return Ok(());
    }

    for task in &tasks {
        task.bind(py).call_method0("cancel")?;
    }

    let gather = asyncio.getattr("gather")?;
    let task_tuple = PyTuple::new(py, tasks.iter())?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("return_exceptions", true)?;
    let wait_all = gather.call(task_tuple, Some(&kwargs))?;
    let _ = loop_ref.call_method1("run_until_complete", (wait_all,))?;

    if let Ok(shutdown_asyncgens) = loop_ref.getattr("shutdown_asyncgens") {
        let _ = loop_ref.call_method1("run_until_complete", (shutdown_asyncgens.call0()?,))?;
    }
    Ok(())
}
