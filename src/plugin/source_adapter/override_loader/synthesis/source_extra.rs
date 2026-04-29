use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use super::super::super::model::*;
use super::super::paths::path_to_forward_slashes;

pub(super) fn inject_source_extra(
    extra: &mut HashMap<String, Value>,
    family: SourcePluginFamily,
    source_path: &Path,
    override_path: &Path,
    source_manifest_name: Option<&str>,
) {
    let (adapter_family, compat_level) = match family {
        SourcePluginFamily::Native => (SourceAdapterFamily::Native, SourceCompatLevel::Native),
        SourcePluginFamily::LiteyukiPy => (
            SourceAdapterFamily::LiteyukiPythonBridge,
            SourceCompatLevel::Bridged,
        ),
        SourcePluginFamily::Astrbot => (
            SourceAdapterFamily::AstrbotPythonBridge,
            SourceCompatLevel::Bridged,
        ),
        SourcePluginFamily::Neomofox => (
            SourceAdapterFamily::NeomofoxPythonBridge,
            SourceCompatLevel::Bridged,
        ),
        SourcePluginFamily::Nonebot => (
            SourceAdapterFamily::NonebotExternal,
            SourceCompatLevel::MetadataOnly,
        ),
    };
    extra.insert(
        EXTRA_SOURCE_FAMILY.to_string(),
        serde_json::to_value(family).unwrap_or_else(|_| Value::String("native".to_string())),
    );
    extra.insert(
        EXTRA_ADAPTER_FAMILY.to_string(),
        serde_json::to_value(adapter_family)
            .unwrap_or_else(|_| Value::String("native".to_string())),
    );
    extra.insert(
        EXTRA_COMPAT_LEVEL.to_string(),
        serde_json::to_value(compat_level).unwrap_or_else(|_| Value::String("native".to_string())),
    );
    extra.insert(
        EXTRA_SOURCE_PATH.to_string(),
        Value::String(path_to_forward_slashes(source_path)),
    );
    extra.insert(
        EXTRA_OVERRIDE_MANIFEST_PATH.to_string(),
        Value::String(path_to_forward_slashes(override_path)),
    );
    if let Some(name) = source_manifest_name {
        extra.insert(
            EXTRA_SOURCE_MANIFEST.to_string(),
            Value::String(name.to_string()),
        );
        extra.insert(
            EXTRA_SOURCE_MANIFEST_PATH.to_string(),
            Value::String(path_to_forward_slashes(&source_path.join(name))),
        );
    }
}
