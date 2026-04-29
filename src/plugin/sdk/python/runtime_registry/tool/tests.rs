use pyo3::Python;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods, PyModule};

use super::decode_python_tool_registration;

#[test]
// 必要测试
fn decode_python_tool_registration_defaults_active_true() {
    Python::with_gil(|py| {
        let types = PyModule::import(py, "types").expect("types");
        let kwargs = PyDict::new(py);
        kwargs.set_item("name", "tool_a").expect("name");
        let item = types
            .getattr("SimpleNamespace")
            .expect("SimpleNamespace")
            .call((), Some(&kwargs))
            .expect("item");

        let registration =
            decode_python_tool_registration("demo", item).expect("tool registration should decode");

        assert_eq!(registration.name, "tool_a");
        assert!(registration.active);
    });
}
