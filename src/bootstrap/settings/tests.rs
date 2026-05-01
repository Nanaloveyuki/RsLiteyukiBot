use super::*;
use liteyukibot_core::test_support::{EnvVarGuard, process_state_lock};

use crate::app_config::{
    AppConfigDoc, AppRustSection, LogConfigSection, RuntimeConfigSection, runtime_settings_values,
};

#[test]
// 必要测试
fn runtime_settings_from_app_config_with_env_prefers_env_over_doc_values() {
    let _lock = process_state_lock();
    let _workers = EnvVarGuard::set("LY_WORKERS", "12");
    let _log_level = EnvVarGuard::set("LY_LOG_LEVEL", "warn");

    let doc = AppConfigDoc {
        rust: Some(AppRustSection {
            runtime: Some(RuntimeConfigSection {
                worker_count: Some(5),
                ingress_queue: Some(2048),
                worker_queue: Some(256),
            }),
            log: None,
            adapters: None,
            tui: None,
            i18n: None,
            commands: None,
            plugins: None,
        }),
        runtime: None,
        log: Some(LogConfigSection {
            mode: Some("mono".to_string()),
            level: Some("error".to_string()),
            timezone: Some("utc".to_string()),
            timestamp_format: None,
            timestamp_pattern: Some("%Y/%m/%d %H:%M:%S".to_string()),
        }),
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        flow_local_agent: None,
        commands: None,
        plugins: None,
        desktop: None,
        onebot_v11: None,
    };

    let settings = RuntimeSettings::from_map_with_env(runtime_settings_values(&doc));

    assert_eq!(settings.runtime_config.worker_count, 12);
    assert_eq!(settings.runtime_config.ingress_queue, 2048);
    assert_eq!(settings.runtime_config.worker_queue, 256);
    assert_eq!(settings.runtime_config.logger.mode, LogMode::Mono);
    assert_eq!(settings.runtime_config.logger.min_level, LogLevel::Warn);
    assert_eq!(settings.runtime_config.logger.timezone, TimeZone::Utc);
    assert_eq!(
        settings.runtime_config.logger.timestamp_format,
        TimestampFormat::Custom("%Y/%m/%d %H:%M:%S".to_string())
    );
}

#[test]
// 必要测试
fn runtime_settings_from_app_config_with_env_reads_root_sections_without_env() {
    let _lock = process_state_lock();
    let _workers = EnvVarGuard::remove("LY_WORKERS");
    let _log_level = EnvVarGuard::remove("LY_LOG_LEVEL");

    let doc = AppConfigDoc {
        rust: None,
        runtime: Some(RuntimeConfigSection {
            worker_count: Some(7),
            ingress_queue: Some(1024),
            worker_queue: Some(128),
        }),
        log: Some(LogConfigSection {
            mode: Some("color".to_string()),
            level: Some("info".to_string()),
            timezone: Some("local".to_string()),
            timestamp_format: Some("epoch_ms".to_string()),
            timestamp_pattern: None,
        }),
        adapters: None,
        connect: None,
        tui: None,
        i18n: None,
        llm: None,
        flow_local_agent: None,
        commands: None,
        plugins: None,
        desktop: None,
        onebot_v11: None,
    };

    let settings = RuntimeSettings::from_map_with_env(runtime_settings_values(&doc));

    assert_eq!(settings.runtime_config.worker_count, 7);
    assert_eq!(settings.runtime_config.ingress_queue, 1024);
    assert_eq!(settings.runtime_config.worker_queue, 128);
    assert_eq!(settings.runtime_config.logger.mode, LogMode::Color);
    assert_eq!(settings.runtime_config.logger.min_level, LogLevel::Info);
    assert_eq!(settings.runtime_config.logger.timezone, TimeZone::Local);
    assert_eq!(
        settings.runtime_config.logger.timestamp_format,
        TimestampFormat::EpochMillis
    );
}
