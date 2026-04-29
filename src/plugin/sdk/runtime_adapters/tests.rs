use serde_json::Value;

use super::*;
use crate::plugin::{PluginMetadata, PluginRuntimeSpec, PluginSdkSpec};

struct TestHost {
    app_version: &'static str,
    api_version: &'static str,
}

impl PluginHostApi for TestHost {
    fn log(&self, _message: String) -> PluginSdkFuture<()> {
        Box::pin(async { Ok(()) })
    }

    fn publish(
        &self,
        _channel_name: String,
        _topic: String,
        _payload: Value,
    ) -> PluginSdkFuture<()> {
        Box::pin(async { Ok(()) })
    }

    fn kv_get(&self, _key: String) -> PluginSdkFuture<Option<Value>> {
        Box::pin(async { Ok(None) })
    }

    fn kv_set(&self, _key: String, _value: Value) -> PluginSdkFuture<()> {
        Box::pin(async { Ok(()) })
    }

    fn host_app_version(&self) -> &str {
        self.app_version
    }

    fn host_api_version(&self) -> &'static str {
        self.api_version
    }
}

fn test_descriptor() -> PluginDescriptor {
    PluginDescriptor {
        metadata: PluginMetadata {
            id: "demo".to_string(),
            name: "Demo".to_string(),
            ..PluginMetadata::default()
        },
        runtime: PluginRuntimeSpec {
            kind: PluginRuntimeKind::Native,
            entrypoint: "main".to_string(),
            ..PluginRuntimeSpec::default()
        },
        sdk: PluginSdkSpec {
            api_version: "0.1".to_string(),
            ..PluginSdkSpec::default()
        },
        ..PluginDescriptor::default()
    }
}

#[test]
// 必要测试
fn build_plugin_contract_rejects_newer_requested_host_api() {
    let mut descriptor = test_descriptor();
    descriptor.sdk.api_version = "0.2".to_string();
    let host = TestHost {
        app_version: "1.0.0",
        api_version: "0.1",
    };

    let error = build_plugin_contract(
        &descriptor,
        PluginRuntimeKind::Native,
        "liteyuki-native",
        &host,
        true,
    )
    .expect_err("higher requested host api should be rejected");

    assert!(matches!(
        error,
        PluginSdkError::UnsupportedRuntime { ref reason, .. }
            if reason.contains("api_version '0.2' is not supported by host api 0.1")
    ));
}

#[test]
// 必要测试
fn build_plugin_contract_rejects_invalid_min_host_version() {
    let mut descriptor = test_descriptor();
    descriptor.sdk.min_host_version = "1.a".to_string();
    let host = TestHost {
        app_version: "1.0.0",
        api_version: "0.1",
    };

    let error = build_plugin_contract(
        &descriptor,
        PluginRuntimeKind::Native,
        "liteyuki-native",
        &host,
        true,
    )
    .expect_err("invalid version string should be rejected");

    assert!(matches!(
        error,
        PluginSdkError::UnsupportedRuntime { ref reason, .. }
            if reason.contains("sdk.min_host_version should use dot-separated numeric versions")
    ));
}
