use std::collections::HashSet;
use std::path::{Path, PathBuf};

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyModule, PyTuple};

pub(super) fn capture_plugin_module_names(
    py: Python<'_>,
    entry_module: &str,
    search_paths: &[PathBuf],
) -> PyResult<Vec<String>> {
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    let builtins = py.import("builtins")?;
    let items = builtins
        .getattr("list")?
        .call1((modules.items(),))?
        .downcast_into::<PyList>()?;
    let mut names = HashSet::new();
    let entry_prefix = format!("{entry_module}.");

    for entry in items.iter() {
        let tuple = entry.downcast_into::<PyTuple>()?;
        let Some(key) = tuple.get_item(0).ok() else {
            continue;
        };
        let Some(value) = tuple.get_item(1).ok() else {
            continue;
        };
        let Ok(name) = key.extract::<String>() else {
            continue;
        };
        if name == entry_module || name.starts_with(entry_prefix.as_str()) {
            names.insert(name);
            continue;
        }
        if module_matches_search_paths(&value, search_paths)? {
            names.insert(name);
        }
    }

    let mut modules = names.into_iter().collect::<Vec<_>>();
    modules.sort();
    Ok(modules)
}

pub(super) fn remove_stale_entrypoint_modules(
    py: Python<'_>,
    entry_module: &str,
    search_paths: &[PathBuf],
) -> PyResult<()> {
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    let builtins = py.import("builtins")?;
    let items = builtins
        .getattr("list")?
        .call1((modules.items(),))?
        .downcast_into::<PyList>()?;
    let entry_prefix = format!("{entry_module}.");
    let mut stale_names = Vec::new();

    for entry in items.iter() {
        let tuple = entry.downcast_into::<PyTuple>()?;
        let Some(key) = tuple.get_item(0).ok() else {
            continue;
        };
        let Some(value) = tuple.get_item(1).ok() else {
            continue;
        };
        let Ok(name) = key.extract::<String>() else {
            continue;
        };
        if (name == entry_module || name.starts_with(entry_prefix.as_str()))
            && !module_matches_search_paths(&value, search_paths)?
        {
            stale_names.push(name);
        }
    }

    for name in stale_names {
        let contains = modules
            .call_method1("__contains__", (name.as_str(),))?
            .is_truthy()?;
        if contains {
            modules.del_item(name.as_str())?;
        }
    }

    Ok(())
}

pub(super) fn import_python_entrypoint_module<'py>(
    py: Python<'py>,
    entry_module: &str,
    search_paths: &[PathBuf],
) -> PyResult<pyo3::Bound<'py, PyModule>> {
    let Some((entry_path, package_dir)) = resolve_entrypoint_path(entry_module, search_paths)
    else {
        return PyModule::import(py, entry_module);
    };

    let importlib_util = py.import("importlib.util")?;
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    let entry_path = entry_path.to_string_lossy().into_owned();
    let spec = if let Some(package_dir) = package_dir {
        let kwargs = PyDict::new(py);
        let locations = PyList::new(py, [package_dir.to_string_lossy().into_owned()])?;
        kwargs.set_item("submodule_search_locations", locations)?;
        importlib_util.call_method(
            "spec_from_file_location",
            (entry_module, entry_path.as_str()),
            Some(&kwargs),
        )?
    } else {
        importlib_util.call_method1(
            "spec_from_file_location",
            (entry_module, entry_path.as_str()),
        )?
    };
    if spec.is_none() {
        return Err(PyRuntimeError::new_err(format!(
            "python entrypoint module '{}' could not be loaded from {}",
            entry_module, entry_path
        )));
    }

    let module = importlib_util.call_method1("module_from_spec", (&spec,))?;
    modules.set_item(entry_module, &module)?;
    let loader = spec.getattr("loader")?;
    if let Err(err) = loader.call_method1("exec_module", (&module,)) {
        let _ = modules.del_item(entry_module);
        return Err(err);
    }
    Ok(module.downcast_into::<PyModule>()?)
}

pub(super) fn remove_python_modules(py: Python<'_>, module_names: &[String]) -> PyResult<()> {
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.downcast_into::<PyDict>()?;
    for name in module_names {
        let contains = modules
            .call_method1("__contains__", (name.as_str(),))?
            .is_truthy()?;
        if contains {
            modules.del_item(name.as_str())?;
        }
    }
    Ok(())
}

pub(super) fn remove_python_search_paths(py: Python<'_>, search_paths: &[PathBuf]) -> PyResult<()> {
    if search_paths.is_empty() {
        return Ok(());
    }
    let sys = py.import("sys")?;
    let py_path = sys.getattr("path")?;
    for path in search_paths {
        let path_text = path.to_string_lossy().into_owned();
        if path_text.trim().is_empty() {
            continue;
        }
        loop {
            let exists = py_path
                .call_method1("__contains__", (path_text.as_str(),))?
                .is_truthy()?;
            if !exists {
                break;
            }
            py_path.call_method1("remove", (path_text.as_str(),))?;
        }
    }
    Ok(())
}

fn resolve_entrypoint_path(
    entry_module: &str,
    search_paths: &[PathBuf],
) -> Option<(PathBuf, Option<PathBuf>)> {
    let module_relative = entry_module.replace('.', std::path::MAIN_SEPARATOR_STR);
    let file_candidate = format!("{module_relative}.py");
    let package_candidate = PathBuf::from(&module_relative).join("__init__.py");

    for base in search_paths {
        let file_path = base.join(file_candidate.as_str());
        if file_path.exists() {
            return Some((file_path, None));
        }
        let package_path = base.join(&package_candidate);
        if package_path.exists() {
            let package_dir = package_path.parent().map(Path::to_path_buf);
            return Some((package_path, package_dir));
        }
    }

    None
}

fn module_matches_search_paths(
    module: &pyo3::Bound<'_, PyAny>,
    search_paths: &[PathBuf],
) -> PyResult<bool> {
    if let Ok(file_attr) = module.getattr("__file__")
        && let Ok(file_path) = file_attr.extract::<String>()
        && path_matches_search_paths(file_path.as_str(), search_paths)
    {
        return Ok(true);
    }

    if let Ok(path_attr) = module.getattr("__path__") {
        for item in path_attr.try_iter()? {
            let item = item?;
            if let Ok(path) = item.extract::<String>()
                && path_matches_search_paths(path.as_str(), search_paths)
            {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn path_matches_search_paths(raw: &str, search_paths: &[PathBuf]) -> bool {
    let path = Path::new(raw);
    search_paths.iter().any(|base| path.starts_with(base))
}
