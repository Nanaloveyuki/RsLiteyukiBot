use std::env;
use std::path::{Path, PathBuf};

use pyo3::types::PyAnyMethods;
use serde_json::Value;

use crate::plugin::PluginDescriptor;

const PYTHON_META_ATTRS: [&str; 3] = [
    "__plugin_meta__",
    "__plugin_metadata__",
    "__liteyuki_plugin_meta__",
];

#[derive(Debug, Clone)]
pub(crate) struct PythonEntrypoint {
    pub(crate) module: String,
    pub(crate) callable: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PythonCompatibilityProbe {
    pub(crate) entrypoint: PythonEntrypoint,
    pub(crate) search_paths: Vec<PathBuf>,
}

pub(crate) fn probe_python_plugin_compatibility(
    descriptor: &PluginDescriptor,
) -> Result<PythonCompatibilityProbe, String> {
    let entrypoint = parse_python_entrypoint(descriptor)?;
    let search_paths = collect_python_search_paths(descriptor);
    validate_entrypoint_module_exists(entrypoint.module.as_str(), search_paths.as_slice())?;
    Ok(PythonCompatibilityProbe {
        entrypoint,
        search_paths,
    })
}

fn parse_python_entrypoint(descriptor: &PluginDescriptor) -> Result<PythonEntrypoint, String> {
    let entrypoint = descriptor.runtime.entrypoint.trim();
    let module_hint = descriptor.runtime.module.trim();
    let raw = if !entrypoint.is_empty() {
        entrypoint
    } else if !module_hint.is_empty() {
        module_hint
    } else {
        return Err(
            "python plugin entrypoint/module is empty; expected `module[:callable]`".to_string(),
        );
    };

    let (module, callable) = match raw.split_once(':') {
        Some((module, callable)) => (
            module.trim().to_string(),
            Some(callable.trim().to_string()).filter(|name| !name.is_empty()),
        ),
        None => (raw.to_string(), None),
    };

    if module.trim().is_empty() {
        return Err("python plugin module name is empty".to_string());
    }
    if raw.contains(':') && callable.is_none() {
        return Err("python plugin callable name is empty".to_string());
    }

    Ok(PythonEntrypoint { module, callable })
}

pub(crate) fn collect_python_search_paths(descriptor: &PluginDescriptor) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let manifest_dir = descriptor
        .manifest_path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    if let Some(dir) = &manifest_dir {
        push_unique_path(&mut paths, dir.clone());
        if let Some(parent) = dir.parent() {
            push_unique_path(&mut paths, parent.to_path_buf());
        }
    }

    for key in ["python_path", "python_paths", "sys_path"] {
        if let Some(value) = descriptor.runtime.options.get(key) {
            extend_python_paths_from_value(value, manifest_dir.as_deref(), &mut paths);
        }
    }

    paths
}

fn validate_entrypoint_module_exists(module: &str, search_paths: &[PathBuf]) -> Result<(), String> {
    let module_relative = module.replace('.', std::path::MAIN_SEPARATOR_STR);
    let file_candidate = format!("{module_relative}.py");
    let package_candidate = PathBuf::from(&module_relative).join("__init__.py");

    let exists = search_paths.iter().any(|base| {
        let file_path = base.join(file_candidate.as_str());
        let package_path = base.join(&package_candidate);
        file_path.exists() || package_path.exists()
    });

    if exists {
        Ok(())
    } else {
        Err(format!(
            "pyo3 compatibility probe failed for '{}': module file was not found in configured search paths",
            module
        ))
    }
}

fn extend_python_paths_from_value(value: &Value, base: Option<&Path>, output: &mut Vec<PathBuf>) {
    match value {
        Value::String(raw) => {
            for item in split_python_path_list(raw) {
                push_unique_path(output, resolve_python_path(item, base));
            }
        }
        Value::Array(list) => {
            for item in list {
                if let Some(raw) = item.as_str() {
                    for path in split_python_path_list(raw) {
                        push_unique_path(output, resolve_python_path(path, base));
                    }
                }
            }
        }
        _ => {}
    }
}

fn split_python_path_list(raw: &str) -> Vec<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let split_paths: Vec<PathBuf> = env::split_paths(trimmed).collect();
    if split_paths.len() > 1 {
        return split_paths;
    }

    if trimmed.contains(',') {
        let parts: Vec<PathBuf> = trimmed
            .split(',')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(PathBuf::from)
            .collect();
        if !parts.is_empty() {
            return parts;
        }
    }

    vec![PathBuf::from(trimmed)]
}

fn resolve_python_path(path: PathBuf, base: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path
    } else if let Some(base) = base {
        base.join(path)
    } else {
        path
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if candidate.as_os_str().is_empty() {
        return;
    }
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
    }
}

pub(crate) fn ensure_python_search_paths(
    py: pyo3::Python<'_>,
    paths: &[PathBuf],
) -> pyo3::PyResult<()> {
    let sys = py.import("sys")?;
    let py_path = sys.getattr("path")?;
    for path in paths.iter().rev() {
        let path_text = path.to_string_lossy().into_owned();
        if path_text.trim().is_empty() {
            continue;
        }
        let exists = py_path
            .call_method1("__contains__", (path_text.as_str(),))?
            .is_truthy()?;
        if !exists {
            py_path.call_method1("insert", (0, path_text.as_str()))?;
        }
    }
    Ok(())
}

pub(crate) fn inspect_python_legacy_metadata(module: &pyo3::Bound<'_, pyo3::types::PyModule>) {
    for attr in PYTHON_META_ATTRS {
        if let Ok(meta) = module.getattr(attr) {
            let _ = meta
                .getattr("name")
                .and_then(|name| name.extract::<String>());
            let _ = meta.getattr("type").and_then(|kind| {
                if let Ok(value) = kind.getattr("value") {
                    value.extract::<String>()
                } else {
                    kind.extract::<String>()
                }
            });
            break;
        }
    }
}
