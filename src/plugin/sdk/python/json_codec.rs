use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use serde_json::Value;

pub(super) fn py_any_to_json(value: &pyo3::Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    let py = value.py();
    let json = py.import("json")?;
    let dumped: String = json.call_method1("dumps", (value,))?.extract()?;
    serde_json::from_str::<Value>(dumped.as_str()).map_err(|err| {
        PyValueError::new_err(format!("python value is not json-serializable: {}", err))
    })
}

pub(super) fn json_to_pyobject(py: Python<'_>, value: &Value) -> PyResult<PyObject> {
    let json = py.import("json")?;
    let dumped = serde_json::to_string(value)
        .map_err(|err| PyValueError::new_err(format!("json serialization failed: {}", err)))?;
    let parsed = json.call_method1("loads", (dumped,))?;
    Ok(parsed.unbind())
}
