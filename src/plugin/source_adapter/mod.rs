mod family;
mod model;
mod override_loader;

use serde_json::Value;

use crate::plugin::{PluginCapabilitySnapshot, PluginDescriptor, PluginRuntimeKind};

pub use override_loader::discover_plugin_manifests_in_dirs;

use self::model::{
    EXTRA_ADAPTER_FAMILY, EXTRA_COMPAT_LEVEL, EXTRA_OVERRIDE_MANIFEST_PATH, EXTRA_SOURCE_FAMILY,
    EXTRA_SOURCE_PATH, SYNTHETIC_MANIFEST_FILENAME, SourceAdapterFamily, SourceCompatLevel,
    SourcePluginFamily,
};

pub(crate) fn descriptor_source_family(
    descriptor: &PluginDescriptor,
) -> Option<SourcePluginFamily> {
    descriptor
        .metadata
        .extra
        .get(EXTRA_SOURCE_FAMILY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub(crate) fn descriptor_adapter_family(
    descriptor: &PluginDescriptor,
) -> Option<SourceAdapterFamily> {
    descriptor
        .metadata
        .extra
        .get(EXTRA_ADAPTER_FAMILY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub(crate) fn descriptor_compat_level(descriptor: &PluginDescriptor) -> Option<SourceCompatLevel> {
    descriptor
        .metadata
        .extra
        .get(EXTRA_COMPAT_LEVEL)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub(crate) fn descriptor_allows_metadata_only_health_check_skip(
    descriptor: &PluginDescriptor,
) -> bool {
    descriptor
        .manifest_path
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        == Some(SYNTHETIC_MANIFEST_FILENAME)
        && matches!(
            descriptor_source_family(descriptor),
            Some(SourcePluginFamily::Nonebot)
        )
        && matches!(
            descriptor_adapter_family(descriptor),
            Some(SourceAdapterFamily::NonebotExternal)
        )
        && matches!(
            descriptor_compat_level(descriptor),
            Some(SourceCompatLevel::MetadataOnly)
        )
        && descriptor.runtime.kind == PluginRuntimeKind::External
        && descriptor.runtime.abi == "nonebot-external"
        && descriptor
            .runtime
            .options
            .get("compat_family")
            .and_then(Value::as_str)
            == Some("nonebot")
        && descriptor
            .runtime
            .options
            .get("source_root")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        && descriptor
            .metadata
            .extra
            .contains_key(EXTRA_SOURCE_PATH)
        && descriptor
            .metadata
            .extra
            .contains_key(EXTRA_OVERRIDE_MANIFEST_PATH)
}

pub(crate) fn descriptor_source_kind(
    descriptor: &PluginDescriptor,
    snapshot: Option<&PluginCapabilitySnapshot>,
) -> &'static str {
    match descriptor_source_family(descriptor) {
        Some(SourcePluginFamily::Native) => "liteyuki-native",
        Some(SourcePluginFamily::LiteyukiPy) => "liteyuki-python-bridge",
        Some(SourcePluginFamily::Astrbot) => "astrbot-compatible",
        Some(SourcePluginFamily::Nonebot) => "nonebot-plugin",
        None => legacy_source_kind(
            descriptor.runtime.kind,
            legacy_compat_kind(descriptor, snapshot),
        ),
    }
}

pub(crate) fn descriptor_compat_kind(
    descriptor: &PluginDescriptor,
    snapshot: Option<&PluginCapabilitySnapshot>,
) -> &'static str {
    match descriptor_adapter_family(descriptor) {
        Some(SourceAdapterFamily::Native) => "native",
        Some(SourceAdapterFamily::LiteyukiPythonBridge) => "liteyuki",
        Some(SourceAdapterFamily::AstrbotPythonBridge) => "astrbot",
        Some(SourceAdapterFamily::NonebotExternal) => match descriptor_compat_level(descriptor) {
            Some(SourceCompatLevel::MetadataOnly) => "metadata-only",
            _ => "nonebot",
        },
        None => legacy_compat_kind(descriptor, snapshot),
    }
}

fn legacy_compat_kind(
    descriptor: &PluginDescriptor,
    snapshot: Option<&PluginCapabilitySnapshot>,
) -> &'static str {
    if descriptor.runtime.kind == PluginRuntimeKind::Python
        && snapshot.is_some_and(plugin_snapshot_has_runtime_capabilities)
    {
        "astrbot"
    } else {
        "none"
    }
}

fn legacy_source_kind(runtime_kind: PluginRuntimeKind, compat_kind: &str) -> &'static str {
    match runtime_kind {
        PluginRuntimeKind::Native => "liteyuki-native",
        PluginRuntimeKind::Python if compat_kind == "astrbot" => "astrbot-compatible",
        PluginRuntimeKind::Python => "liteyuki-python-bridge",
        PluginRuntimeKind::Lua => "runtime-lua",
        PluginRuntimeKind::External => "runtime-external",
    }
}

fn plugin_snapshot_has_runtime_capabilities(snapshot: &PluginCapabilitySnapshot) -> bool {
    !snapshot.tools.is_empty()
        || !snapshot.web_apis.is_empty()
        || !snapshot.cron_jobs.is_empty()
        || !snapshot.tasks.is_empty()
}

pub(crate) fn descriptor_family_value(descriptor: &PluginDescriptor, key: &str) -> Option<Value> {
    descriptor.metadata.extra.get(key).cloned()
}
