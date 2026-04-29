use pyo3::Python;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods, PyModule};

use super::decode_python_cron_registration;

#[test]
// 必要测试
fn decode_python_cron_registration_reports_missing_handler_attr() {
    Python::with_gil(|py| {
        let types = PyModule::import(py, "types").expect("types");
        let kwargs = PyDict::new(py);
        kwargs.set_item("job_id", "job_a").expect("job_id");
        let item = types
            .getattr("SimpleNamespace")
            .expect("SimpleNamespace")
            .call((), Some(&kwargs))
            .expect("item");

        let error = match decode_python_cron_registration("demo", item) {
            Ok(_) => panic!("missing handler should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("cron handler lookup failed"));
    });
}
