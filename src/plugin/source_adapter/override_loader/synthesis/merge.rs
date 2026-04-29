use crate::plugin::{PluginRuntimeSpec, PluginSdkSpec};

use super::super::super::model::{RuntimeOverrideDoc, SdkOverrideDoc};

pub(super) fn merge_runtime(
    mut base: PluginRuntimeSpec,
    override_doc: RuntimeOverrideDoc,
) -> PluginRuntimeSpec {
    if let Some(kind) = override_doc.kind {
        base.kind = kind;
    }
    if let Some(entrypoint) = override_doc.entrypoint {
        base.entrypoint = entrypoint;
    }
    if let Some(module) = override_doc.module {
        base.module = module;
    }
    if let Some(abi) = override_doc.abi {
        base.abi = abi;
    }
    if let Some(min_version) = override_doc.min_version {
        base.min_version = min_version;
    }
    if let Some(options) = override_doc.options {
        base.options.extend(options);
    }
    base
}

pub(super) fn merge_sdk(mut base: PluginSdkSpec, override_doc: SdkOverrideDoc) -> PluginSdkSpec {
    if let Some(api_version) = override_doc.api_version {
        base.api_version = api_version;
    }
    if let Some(min_host_version) = override_doc.min_host_version {
        base.min_host_version = min_host_version;
    }
    if let Some(options) = override_doc.options {
        base.options.extend(options);
    }
    base
}
