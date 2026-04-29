use std::cmp::Ordering;
use std::sync::Arc;

use super::super::abi::{PluginAbiContract, PluginAbiMethod};
use super::super::{PluginDescriptor, PluginRuntimeKind};
use super::host_bridge::{PluginHostApi, PluginSdkFuture, default_host_api_version};
use super::python::probe::probe_python_plugin_compatibility;
use super::{PluginPermissionSet, PluginSdkError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginLoadState {
    Ready,
    Deferred,
}

#[derive(Debug, Clone)]
pub struct PluginLoadPlan {
    pub runtime_kind: PluginRuntimeKind,
    pub state: PluginLoadState,
    pub reason: Option<String>,
    pub contract: PluginAbiContract,
}

impl PluginLoadPlan {
    pub fn ready(runtime_kind: PluginRuntimeKind, contract: PluginAbiContract) -> Self {
        Self {
            runtime_kind,
            state: PluginLoadState::Ready,
            reason: None,
            contract,
        }
    }

    pub fn deferred(
        runtime_kind: PluginRuntimeKind,
        contract: PluginAbiContract,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            runtime_kind,
            state: PluginLoadState::Deferred,
            reason: Some(reason.into()),
            contract,
        }
    }
}

pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> PluginRuntimeKind;
    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan>;
}

#[derive(Clone, Default)]
pub struct RuntimeAdapterRegistry {
    adapters: Vec<Arc<dyn RuntimeAdapter>>,
}

impl RuntimeAdapterRegistry {
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        registry.register(NativeRuntimeAdapter);
        registry.register(PythonRuntimeAdapter);
        registry.register(LuaRuntimeAdapter);
        registry.register(ExternalRuntimeAdapter);
        registry
    }

    pub fn register<A: RuntimeAdapter + 'static>(&mut self, adapter: A) {
        self.adapters.push(Arc::new(adapter));
    }

    pub fn find(&self, kind: PluginRuntimeKind) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.kind() == kind)
            .cloned()
    }
}

pub struct NativeRuntimeAdapter;
pub struct PythonRuntimeAdapter;
pub struct LuaRuntimeAdapter;
pub struct ExternalRuntimeAdapter;

impl RuntimeAdapter for NativeRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Native
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = build_plugin_contract(
            descriptor,
            PluginRuntimeKind::Native,
            "liteyuki-native",
            host,
            true,
        );
        let has_entry = !descriptor.runtime.entrypoint.trim().is_empty()
            || !descriptor.runtime.module.trim().is_empty();
        Box::pin(async move {
            let contract = contract?;
            if has_entry {
                Ok(PluginLoadPlan::ready(PluginRuntimeKind::Native, contract))
            } else {
                Ok(PluginLoadPlan::deferred(
                    PluginRuntimeKind::Native,
                    contract,
                    "native plugin entrypoint is not declared",
                ))
            }
        })
    }
}

impl RuntimeAdapter for PythonRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Python
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = build_plugin_contract(
            descriptor,
            PluginRuntimeKind::Python,
            "liteyuki-python-bridge",
            host,
            true,
        );
        let probe = probe_python_plugin_compatibility(descriptor);
        Box::pin(async move {
            let contract = contract?;
            match probe {
                Ok(_) => Ok(PluginLoadPlan::ready(PluginRuntimeKind::Python, contract)),
                Err(reason) => Ok(PluginLoadPlan::deferred(
                    PluginRuntimeKind::Python,
                    contract,
                    reason,
                )),
            }
        })
    }
}

impl RuntimeAdapter for LuaRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::Lua
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = build_plugin_contract(
            descriptor,
            PluginRuntimeKind::Lua,
            "liteyuki-lua-bridge",
            host,
            false,
        );
        Box::pin(async move {
            let contract = contract?;
            Ok(PluginLoadPlan::deferred(
                PluginRuntimeKind::Lua,
                contract,
                "lua runtime bridge is reserved for future lua integration",
            ))
        })
    }
}

impl RuntimeAdapter for ExternalRuntimeAdapter {
    fn kind(&self) -> PluginRuntimeKind {
        PluginRuntimeKind::External
    }

    fn plan_load(
        &self,
        descriptor: &PluginDescriptor,
        host: &dyn PluginHostApi,
    ) -> PluginSdkFuture<PluginLoadPlan> {
        let contract = build_plugin_contract(
            descriptor,
            PluginRuntimeKind::External,
            "liteyuki-external-bridge",
            host,
            false,
        );
        Box::pin(async move {
            let contract = contract?;
            Ok(PluginLoadPlan::deferred(
                PluginRuntimeKind::External,
                contract,
                "external runtime bridge is reserved for managed sidecar integration",
            ))
        })
    }
}

fn normalize_abi_version(raw: &str) -> String {
    if raw.trim().is_empty() {
        "1.0".to_string()
    } else {
        raw.trim().to_string()
    }
}

fn build_plugin_contract(
    descriptor: &PluginDescriptor,
    runtime_kind: PluginRuntimeKind,
    abi_name: &str,
    host: &dyn PluginHostApi,
    requires_handle_event: bool,
) -> Result<PluginAbiContract, PluginSdkError> {
    validate_declared_permissions(descriptor.permissions.as_slice(), runtime_kind)?;

    let host_api_version = normalize_host_api_version(host.host_api_version());
    let requested_api_version = parse_version_components(
        descriptor.sdk.api_version.as_str(),
        Some(default_host_api_version()),
        runtime_kind,
        "sdk.api_version",
    )?;
    let host_api_components = parse_version_components(
        host_api_version.as_str(),
        Some(default_host_api_version()),
        runtime_kind,
        "host api version",
    )?;
    let requested_display = format_version_components(requested_api_version.as_slice());
    let host_api_display = format_version_components(host_api_components.as_slice());
    if requested_api_version.first().copied().unwrap_or_default()
        != host_api_components.first().copied().unwrap_or_default()
        || compare_version_components(
            host_api_components.as_slice(),
            requested_api_version.as_slice(),
        ) == Ordering::Less
    {
        return Err(PluginSdkError::UnsupportedRuntime {
            kind: runtime_kind,
            reason: format!(
                "plugin SDK api_version '{}' is not supported by host api {}",
                requested_display, host_api_display
            ),
        });
    }

    if !descriptor.sdk.min_host_version.trim().is_empty() {
        let minimum_host = parse_version_components(
            descriptor.sdk.min_host_version.as_str(),
            None,
            runtime_kind,
            "sdk.min_host_version",
        )?;
        let actual_host = parse_version_components(
            host.host_app_version(),
            None,
            runtime_kind,
            "host app version",
        )?;
        if compare_version_components(actual_host.as_slice(), minimum_host.as_slice())
            == Ordering::Less
        {
            return Err(PluginSdkError::UnsupportedRuntime {
                kind: runtime_kind,
                reason: format!(
                    "plugin requires host version >= {} but current host is {}",
                    format_version_components(minimum_host.as_slice()),
                    format_version_components(actual_host.as_slice())
                ),
            });
        }
    }

    let mut contract = PluginAbiContract::new(
        runtime_kind,
        abi_name,
        normalize_abi_version(&descriptor.runtime.abi),
        host_api_version,
    );
    if requires_handle_event {
        contract.required_methods.push(PluginAbiMethod::HandleEvent);
    }
    Ok(contract)
}

fn validate_declared_permissions(
    permissions: &[String],
    runtime_kind: PluginRuntimeKind,
) -> Result<(), PluginSdkError> {
    PluginPermissionSet::from_declared(permissions)
        .map(|_| ())
        .map_err(|err| PluginSdkError::UnsupportedRuntime {
            kind: runtime_kind,
            reason: err,
        })
}

fn normalize_host_api_version(raw: &str) -> String {
    if raw.trim().is_empty() {
        default_host_api_version().to_string()
    } else {
        raw.trim().to_string()
    }
}

fn parse_version_components(
    raw: &str,
    default_value: Option<&str>,
    runtime_kind: PluginRuntimeKind,
    field_name: &str,
) -> Result<Vec<u64>, PluginSdkError> {
    let candidate = if raw.trim().is_empty() {
        default_value.unwrap_or("")
    } else {
        raw.trim()
    };
    let candidate = candidate
        .split(['-', '+'])
        .next()
        .unwrap_or(candidate)
        .trim();
    if candidate.is_empty() {
        return Err(PluginSdkError::UnsupportedRuntime {
            kind: runtime_kind,
            reason: format!("{field_name} should not be empty"),
        });
    }

    let mut components = Vec::new();
    for segment in candidate.split('.') {
        if segment.is_empty() || !segment.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(PluginSdkError::UnsupportedRuntime {
                kind: runtime_kind,
                reason: format!("{field_name} should use dot-separated numeric versions"),
            });
        }
        let value = segment
            .parse::<u64>()
            .map_err(|_| PluginSdkError::UnsupportedRuntime {
                kind: runtime_kind,
                reason: format!("{field_name} contains an out-of-range version segment"),
            })?;
        components.push(value);
    }

    while components.len() > 1 && components.last() == Some(&0) {
        components.pop();
    }
    Ok(components)
}

fn compare_version_components(left: &[u64], right: &[u64]) -> Ordering {
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let lhs = left.get(index).copied().unwrap_or(0);
        let rhs = right.get(index).copied().unwrap_or(0);
        match lhs.cmp(&rhs) {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }
    Ordering::Equal
}

fn format_version_components(components: &[u64]) -> String {
    components
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
#[path = "runtime_adapters/tests.rs"]
mod tests;
