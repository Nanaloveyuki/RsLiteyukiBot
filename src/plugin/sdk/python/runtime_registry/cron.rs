use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::plugin::sdk::PluginSdkError;
use crate::plugin::sdk::python::bridge_contract::{
    PYTHON_CRON_ATTR_ENABLED, PYTHON_CRON_ATTR_HANDLER, PYTHON_CRON_ATTR_JOB_ID,
};

pub(crate) struct PythonCronRegistration<'py> {
    pub(crate) job_id: String,
    pub(crate) enabled: bool,
    pub(crate) handler: Bound<'py, PyAny>,
}

pub(crate) fn decode_python_cron_registration<'py>(
    plugin_id: &str,
    item: Bound<'py, PyAny>,
) -> Result<PythonCronRegistration<'py>, PluginSdkError> {
    let job_id = item
        .getattr(PYTHON_CRON_ATTR_JOB_ID)
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' cron job id lookup failed: {}",
                plugin_id, err
            ))
        })?
        .extract::<String>()
        .map_err(|err| {
            PluginSdkError::Runtime(format!(
                "python plugin '{}' cron job id decode failed: {}",
                plugin_id, err
            ))
        })?;
    let enabled = item
        .getattr(PYTHON_CRON_ATTR_ENABLED)
        .ok()
        .and_then(|value| value.extract::<bool>().ok())
        .unwrap_or(true);
    let handler = item.getattr(PYTHON_CRON_ATTR_HANDLER).map_err(|err| {
        PluginSdkError::Runtime(format!(
            "python plugin '{}' cron handler lookup failed: {}",
            plugin_id, err
        ))
    })?;
    Ok(PythonCronRegistration {
        job_id,
        enabled,
        handler,
    })
}

#[cfg(test)]
#[path = "cron/tests.rs"]
mod tests;
