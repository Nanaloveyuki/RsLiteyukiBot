use pyo3::Python;
use pyo3::types::PyDict;
use pyo3::types::PyDictMethods;

use super::load_python_tool_registry;
use crate::plugin::sdk::python::bridge_contract::ASTRBOT_RUNTIME_KEY_TOOLS;

#[test]
// 必要测试
fn load_python_runtime_registry_rejects_non_list_registry() {
    Python::with_gil(|py| {
        let runtime = PyDict::new(py);
        runtime
            .set_item(ASTRBOT_RUNTIME_KEY_TOOLS, 42)
            .expect("registry entry");

        let error =
            load_python_tool_registry(&runtime, "demo").expect_err("non-list registry should fail");

        assert!(error.to_string().contains("tool registry is invalid"));
    });
}
