use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::app_config::{
    load_adapter_configs, prime_reload_warning_state, resolve_app_locale, resolve_disabled_plugins,
    resolve_disabled_scope_commands, resolve_help_whitelist, resolve_llm_config,
    resolve_tui_config, runtime_settings_values,
};
use crate::i18n::{reload_catalog as reload_i18n_catalog, set_current_locale, trf};
use crate::utils::config_path::resolve_preferred_password_config_path;
use liteyukibot_core::{BotRuntimeConfig, RuntimeSettings, RuntimeTarget};

use super::{
    ExternalGateway, LlmCommandRuntime, PreparedRuntimeBootstrap, SuperuserManager,
    ensure_default_llm_config_file, load_app_config_with_llm_overlay, resolve_builtin_plugin_dirs,
};

pub(crate) fn prepare_runtime_bootstrap<F>(
    target: RuntimeTarget,
    mut report_startup_warning: F,
) -> Result<PreparedRuntimeBootstrap, String>
where
    F: FnMut(String),
{
    ensure_runtime_bootstrap_files(&mut report_startup_warning);

    let (app_config, mut warnings) = load_app_config_with_llm_overlay();
    prime_reload_warning_state(&app_config);
    let settings = RuntimeSettings::from_map_with_env(runtime_settings_values(&app_config));
    let runtime_config = settings.runtime_config.clone();
    let _ = settings.install_global();
    let effective_runtime_config = target.tune_runtime_config(runtime_config.clone());
    let adapter_configs = load_adapter_configs(&app_config)
        .map_err(|err| format!("failed to load adapters: {err}"))?;
    let adapter_autostart = !adapter_configs.is_empty();
    let help_whitelist = Arc::new(RwLock::new(resolve_help_whitelist(&app_config)));
    let tui_config = resolve_tui_config(&app_config);
    let locale = resolve_app_locale(&app_config);
    let llm_config = resolve_llm_config(&app_config);
    let disabled_commands = resolve_disabled_scope_commands(&app_config);
    let disabled_plugins = resolve_disabled_plugins(&app_config);
    let llm_runtime = LlmCommandRuntime::new(llm_config.command_prefix.clone());
    let external_gateway = ExternalGateway::new();
    let plugin_dirs = resolve_builtin_plugin_dirs();
    set_current_locale(locale);
    warnings.extend(reload_i18n_catalog(plugin_dirs.iter()));
    warnings = dedup_warnings(warnings);

    let superuser_manager =
        match SuperuserManager::load_or_init(resolve_password_config_path().as_path()) {
            Ok(manager) => manager,
            Err(err) => {
                let err_text = err.to_string();
                report_startup_warning(
                    trf(
                        "startup.password_config_fallback",
                        &[("err", err_text.as_str())],
                    )
                    .to_string(),
                );
                SuperuserManager::in_memory()
            }
        };

    Ok(PreparedRuntimeBootstrap {
        warnings,
        runtime_config,
        effective_runtime_config,
        adapter_configs,
        adapter_autostart,
        help_whitelist,
        tui_config,
        locale: locale.as_str().to_string(),
        llm_runtime,
        external_gateway,
        plugin_dirs,
        disabled_commands,
        disabled_plugins,
        superuser_manager,
    })
}

fn ensure_runtime_bootstrap_files<F>(report_startup_warning: &mut F)
where
    F: FnMut(String),
{
    if let Err(err) = crate::app_config::ensure_default_config_files() {
        report_startup_warning(
            trf(
                "startup.ensure_default_config_failed",
                &[("err", err.to_string().as_str())],
            )
            .to_string(),
        );
    }
    if let Err(err) = ensure_default_llm_config_file() {
        report_startup_warning(
            trf(
                "startup.ensure_default_llm_config_failed",
                &[("err", err.as_str())],
            )
            .to_string(),
        );
    }
}

pub(crate) fn resolve_password_config_path() -> PathBuf {
    resolve_preferred_password_config_path()
}

pub(crate) fn describe_runtime_config(runtime_config: &BotRuntimeConfig) -> String {
    RuntimeSettings::describe_runtime_config(runtime_config)
}

pub(crate) fn dedup_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for warning in warnings {
        if seen.insert(warning.clone()) {
            output.push(warning);
        }
    }
    output
}
