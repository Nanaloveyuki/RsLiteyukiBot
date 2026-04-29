use std::path::PathBuf;

use crate::app_config::{
    collect_runtime_reload_warnings, load_adapter_configs, resolve_app_config_path,
    resolve_app_locale, resolve_disabled_plugins, resolve_disabled_scope_commands,
    resolve_help_whitelist, resolve_llm_config, resolve_tui_config,
    write_default_config_if_missing,
};
use crate::config_edit;
use crate::i18n::{reload_catalog as reload_i18n_catalog, set_current_locale, tr, trf};
use crate::runtime_support::{load_app_config_with_llm_overlay, resolve_llm_config_path};
use crate::tui;
use crate::utils::config_path::resolve_default_app_config_path;
use liteyukibot_core::{LiteyukiBot, LogLevel, emit_console_log};

pub(crate) fn reload_from_config(bot: &LiteyukiBot) -> tui::ReloadFuture<'_> {
    Box::pin(async move {
        emit_console_log(
            LogLevel::Info,
            "runtime.reload",
            format!(
                "starting reload (app_config={}, llm_config={})",
                describe_optional_path_for_log(resolve_app_config_path()),
                describe_optional_path_for_log(resolve_llm_config_path())
            ),
        );
        let (app_config, mut warnings) = load_app_config_with_llm_overlay();
        warnings.extend(collect_runtime_reload_warnings(&app_config));
        let adapters = load_adapter_configs(&app_config).map_err(|err| {
            let message = format!("failed to load adapter configs: {err}");
            emit_console_log(
                LogLevel::Error,
                "runtime.reload",
                format!("reload failed during adapter config load: {message}"),
            );
            message
        })?;
        let autostart = !adapters.is_empty();
        let locale = resolve_app_locale(&app_config);
        bot.reload_adapters(adapters.clone(), autostart)
            .await
            .map_err(|err| {
                let message = format!("failed to apply adapter reload: {err}");
                emit_console_log(
                    LogLevel::Error,
                    "runtime.reload",
                    format!("reload failed during adapter apply: {message}"),
                );
                message
            })?;
        let tui_config = resolve_tui_config(&app_config);
        let llm_command_prefix = resolve_llm_config(&app_config).command_prefix;
        let disabled_commands = resolve_disabled_scope_commands(&app_config);
        let disabled_plugins = resolve_disabled_plugins(&app_config);
        let mut help_whitelist: Vec<String> =
            resolve_help_whitelist(&app_config).into_iter().collect();
        help_whitelist.sort();
        bot.reload_plugins(disabled_plugins.clone())
            .await
            .map_err(|err| {
                let message = format!("failed to apply plugin reload: {err}");
                emit_console_log(
                    LogLevel::Error,
                    "runtime.reload",
                    format!("reload failed during plugin apply: {message}"),
                );
                message
            })?;
        set_current_locale(locale);
        warnings.extend(reload_i18n_catalog(bot.plugin_dirs().iter()));
        emit_console_log(
            LogLevel::Info,
            "runtime.reload",
            format!(
                "reload applied (adapters={}, autostart={}, locale={}, help_whitelist={}, llm_prefix={}, disabled_commands={}, disabled_plugins={}, warnings={})",
                adapters.len(),
                autostart,
                locale.as_str(),
                help_whitelist.len(),
                llm_command_prefix,
                disabled_commands.len(),
                disabled_plugins.len(),
                warnings.len()
            ),
        );
        Ok(tui::ReloadResult {
            adapters,
            adapter_autostart: autostart,
            tui_config,
            locale,
            help_whitelist,
            llm_command_prefix,
            disabled_commands,
            disabled_plugins,
            warnings,
        })
    })
}

pub(crate) fn apply_plugin_policy(
    bot: &LiteyukiBot,
    disabled_plugins: Vec<String>,
) -> tui::PluginPolicyFuture<'_> {
    Box::pin(async move {
        bot.reload_plugins(disabled_plugins)
            .await
            .map_err(|err| format!("failed to apply plugin reload: {err}"))?;
        let warnings = reload_i18n_catalog(bot.plugin_dirs().iter());
        let disabled_count = bot.disabled_plugin_ids().len().to_string();
        let mut message = tr("plugin.reload.success").replace("{count}", disabled_count.as_str());
        if !warnings.is_empty() {
            message.push_str(" | ");
            message.push_str(warnings.join(" | ").as_str());
        }
        Ok(message)
    })
}

pub(crate) fn persist_help_whitelist(entries: Vec<String>) -> Result<String, String> {
    persist_app_config_update(
        "whitelist.persist.success",
        entries,
        config_edit::persist_onebot_v11_whitelist,
    )
}

pub(crate) fn persist_disabled_commands_config(entries: Vec<String>) -> Result<String, String> {
    persist_app_config_update(
        "command_policy.persist.success",
        entries,
        config_edit::persist_disabled_commands,
    )
}

pub(crate) fn persist_disabled_plugins_config(entries: Vec<String>) -> Result<String, String> {
    persist_app_config_update(
        "plugin_policy.persist.success",
        entries,
        config_edit::persist_disabled_plugins,
    )
}

fn persist_app_config_update(
    message_key: &str,
    entries: Vec<String>,
    persist: fn(&std::path::Path, &[String]) -> Result<(), String>,
) -> Result<String, String> {
    let path = resolve_app_config_path().unwrap_or_else(resolve_default_app_config_path);
    write_default_config_if_missing(path.as_path())
        .map_err(|err| format!("failed to ensure config exists: {err}"))?;
    persist(path.as_path(), &entries)?;
    let path_display = path.display().to_string();
    let count = entries.len().to_string();
    Ok(trf(
        message_key,
        &[("path", path_display.as_str()), ("count", count.as_str())],
    )
    .to_string())
}

fn describe_optional_path_for_log(path: Option<PathBuf>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "<not found>".to_string())
}

#[cfg(test)]
#[path = "reload_service/tests.rs"]
mod tests;
