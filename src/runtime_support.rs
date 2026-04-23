use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::app_config::{
    AppConfigDoc, LlmConfigSection, load_adapter_configs, load_app_config_from_path,
    load_app_config_with_warnings, prime_reload_warning_state, resolve_app_locale,
    resolve_disabled_plugins, resolve_disabled_scope_commands, resolve_help_whitelist,
    resolve_llm_config, resolve_tui_config, validate_app_config,
};
use crate::i18n::{reload_catalog as reload_i18n_catalog, set_current_locale, trf};
use crate::onebot_support::value_to_string;
use crate::superuser::SuperuserManager;
use crate::tui;
use liteyukibot_core::{AdapterConfig, RuntimeSettings, RuntimeTarget};
use liteyukibot_core::{BotRuntimeConfig, LogLevel, LogMode, TimeZone, TimestampFormat};

pub(crate) const EXTERNAL_API_TIMEOUT: Duration = Duration::from_secs(12);
pub(crate) const LLM_CONFIG_PATHS: [&str; 2] = ["llm-config.yaml", "llm-config.toml"];
pub(crate) const LLM_PROMPT_STORE_PATH: &str = "llm-prompts.json";
pub(crate) const PASSWORD_CONFIG_PATH: &str = "password.yaml";
pub(crate) const BUILTIN_PLUGIN_DIRS: [&str; 2] = ["builtin_plugin", "resources/builtin_plugin"];
pub(crate) const DEV_BUILTIN_PLUGIN_DIRS: [&str; 1] = ["src/builtin_plugin"];

static LLM_API_KEY_ROUND_ROBIN: AtomicU64 = AtomicU64::new(0);

pub(crate) struct PreparedRuntimeBootstrap {
    pub(crate) warnings: Vec<String>,
    pub(crate) runtime_config: BotRuntimeConfig,
    pub(crate) effective_runtime_config: BotRuntimeConfig,
    pub(crate) adapter_configs: Vec<AdapterConfig>,
    pub(crate) adapter_autostart: bool,
    pub(crate) help_whitelist: Arc<RwLock<HashSet<String>>>,
    #[allow(dead_code)]
    pub(crate) tui_config: tui::TuiConfig,
    #[allow(dead_code)]
    pub(crate) locale: String,
    pub(crate) llm_runtime: LlmCommandRuntime,
    pub(crate) external_gateway: ExternalGateway,
    pub(crate) plugin_dirs: Vec<PathBuf>,
    pub(crate) disabled_commands: Vec<String>,
    pub(crate) disabled_plugins: Vec<String>,
    pub(crate) superuser_manager: SuperuserManager,
}

#[derive(Clone)]
pub(crate) struct LlmCommandRuntime {
    command_prefix: Arc<RwLock<String>>,
}

impl LlmCommandRuntime {
    pub(crate) fn new(command_prefix: impl Into<String>) -> Self {
        Self {
            command_prefix: Arc::new(RwLock::new(command_prefix.into())),
        }
    }

    pub(crate) fn command_prefix(&self) -> String {
        self.command_prefix
            .read()
            .expect("llm command prefix lock should not be poisoned")
            .clone()
    }

    #[allow(dead_code)]
    pub(crate) fn shared_command_prefix(&self) -> Arc<RwLock<String>> {
        self.command_prefix.clone()
    }

    #[allow(dead_code)]
    pub(crate) fn set_command_prefix(&self, command_prefix: impl Into<String>) {
        *self
            .command_prefix
            .write()
            .expect("llm command prefix lock should not be poisoned") = command_prefix.into();
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ExternalGatewaySnapshot {
    pub(crate) command_hits: u64,
    pub(crate) api_requests: u64,
    pub(crate) api_success: u64,
    pub(crate) api_failed: u64,
    pub(crate) api_timeouts: u64,
    pub(crate) api_inflight: usize,
}

#[derive(Debug)]
struct PendingApiCall {
    started_at: Instant,
}

#[derive(Debug, Default)]
struct ExternalGatewayState {
    command_hits: u64,
    api_requests: u64,
    api_success: u64,
    api_failed: u64,
    api_timeouts: u64,
    pending: HashMap<String, PendingApiCall>,
}

impl ExternalGatewayState {
    fn snapshot(&self) -> ExternalGatewaySnapshot {
        ExternalGatewaySnapshot {
            command_hits: self.command_hits,
            api_requests: self.api_requests,
            api_success: self.api_success,
            api_failed: self.api_failed,
            api_timeouts: self.api_timeouts,
            api_inflight: self.pending.len(),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ExternalGateway {
    state: Arc<Mutex<ExternalGatewayState>>,
    echo_seq: Arc<AtomicU64>,
}

impl ExternalGateway {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn snapshot(&self) -> ExternalGatewaySnapshot {
        self.state
            .lock()
            .expect("external gateway lock should not be poisoned")
            .snapshot()
    }

    pub(crate) fn next_echo(&self, prefix: &str) -> String {
        let seq = self.echo_seq.fetch_add(1, Ordering::SeqCst);
        format!("{prefix}-{seq}")
    }

    pub(crate) fn record_command_hit(&self) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        state.command_hits = state.command_hits.saturating_add(1);
        state.snapshot()
    }

    pub(crate) fn track_request(&self, echo: String) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        state.api_requests = state.api_requests.saturating_add(1);
        state.pending.insert(
            echo,
            PendingApiCall {
                started_at: Instant::now(),
            },
        );
        state.snapshot()
    }

    pub(crate) fn mark_send_failed(&self, echo: &str) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        if state.pending.remove(echo).is_some() {
            state.api_failed = state.api_failed.saturating_add(1);
        }
        state.snapshot()
    }

    pub(crate) fn observe_payload(
        &self,
        payload: &Value,
        timeout: Duration,
    ) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        sweep_pending_timeouts(&mut state, timeout);

        if let Some((echo, success)) = parse_onebot_v11_api_response(payload)
            && state.pending.remove(&echo).is_some()
        {
            if success {
                state.api_success = state.api_success.saturating_add(1);
            } else {
                state.api_failed = state.api_failed.saturating_add(1);
            }
        }

        state.snapshot()
    }

    pub(crate) fn sweep_timeouts(&self, timeout: Duration) -> ExternalGatewaySnapshot {
        let mut state = self
            .state
            .lock()
            .expect("external gateway lock should not be poisoned");
        sweep_pending_timeouts(&mut state, timeout);
        state.snapshot()
    }
}

fn sweep_pending_timeouts(state: &mut ExternalGatewayState, timeout: Duration) {
    let expired: Vec<String> = state
        .pending
        .iter()
        .filter_map(|(echo, call)| {
            if call.started_at.elapsed() >= timeout {
                Some(echo.clone())
            } else {
                None
            }
        })
        .collect();

    if expired.is_empty() {
        return;
    }

    for echo in expired {
        if state.pending.remove(&echo).is_some() {
            state.api_timeouts = state.api_timeouts.saturating_add(1);
        }
    }
}

fn parse_onebot_v11_api_response(payload: &Value) -> Option<(String, bool)> {
    let object = payload.as_object()?;
    if !object.contains_key("status") && !object.contains_key("retcode") {
        return None;
    }

    let echo = object.get("echo").and_then(value_to_string)?;
    let success = object
        .get("status")
        .and_then(Value::as_str)
        .map(|status| status.eq_ignore_ascii_case("ok"))
        .or_else(|| {
            object
                .get("retcode")
                .and_then(Value::as_i64)
                .map(|code| code == 0)
        })
        .unwrap_or(false);
    Some((echo, success))
}

pub(crate) fn next_llm_api_key_index(key_count: usize) -> Option<usize> {
    if key_count == 0 {
        return None;
    }
    Some(LLM_API_KEY_ROUND_ROBIN.fetch_add(1, Ordering::SeqCst) as usize % key_count)
}

pub(crate) fn prepare_runtime_bootstrap<F>(
    target: RuntimeTarget,
    mut report_startup_warning: F,
) -> Result<PreparedRuntimeBootstrap, String>
where
    F: FnMut(String),
{
    ensure_runtime_bootstrap_files(&mut report_startup_warning);

    let settings = load_runtime_settings_with_fallback(&mut report_startup_warning);
    let _ = settings.clone().install_global();
    let mut runtime_config = RuntimeSettings::global_runtime_config().clone();
    runtime_config.logger.min_level = LogLevel::Error;

    let (app_config, mut warnings) = load_app_config_with_llm_overlay();
    prime_reload_warning_state(&app_config);
    apply_runtime_log_overrides_from_app_config(&mut runtime_config, &app_config);
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

fn load_runtime_settings_with_fallback<F>(report_startup_warning: &mut F) -> RuntimeSettings
where
    F: FnMut(String),
{
    match RuntimeSettings::try_load() {
        Ok(settings) => settings,
        Err(err) => {
            report_startup_warning(
                trf(
                    "startup.runtime_settings_fallback",
                    &[("err", err.to_string().as_str())],
                )
                .to_string(),
            );
            RuntimeSettings::default()
        }
    }
}

pub(crate) fn resolve_password_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("LY_PASSWORD_PATH")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    PathBuf::from(PASSWORD_CONFIG_PATH)
}

pub(crate) fn resolve_user_home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

pub(crate) fn resolve_local_plugin_dir() -> PathBuf {
    resolve_user_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".liteyuki")
        .join("plugins")
}

pub(crate) fn resolve_builtin_plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(raw) = std::env::var("LY_PLUGIN_DIRS") {
        for path in std::env::split_paths(&raw) {
            push_explicit_plugin_dir_candidates(&mut dirs, &mut seen, path.as_path());
        }
    }

    push_unique_plugin_path(&mut dirs, &mut seen, resolve_local_plugin_dir());

    if let Ok(current_dir) = std::env::current_dir() {
        push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, current_dir.as_path(), true);
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent()
    {
        push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, parent, false);
    }

    dirs
}

pub(crate) fn push_explicit_plugin_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    path: &std::path::Path,
) {
    push_unique_plugin_path(dirs, seen, path.to_path_buf());
    push_runtime_plugin_dir_candidates(dirs, seen, path, true);
}

pub(crate) fn push_runtime_plugin_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    root: &std::path::Path,
    include_dev_fallback: bool,
) {
    for candidate in BUILTIN_PLUGIN_DIRS {
        push_unique_plugin_path(dirs, seen, root.join(candidate));
    }
    if include_dev_fallback {
        for candidate in DEV_BUILTIN_PLUGIN_DIRS {
            push_unique_plugin_path(dirs, seen, root.join(candidate));
        }
    }
}

fn push_unique_plugin_path(dirs: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        dirs.push(path);
    }
}

pub(crate) fn ensure_default_llm_config_file() -> Result<(), String> {
    if let Ok(path) = std::env::var("LY_LLM_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return ensure_llm_config_file(std::path::Path::new(path.trim()));
    }

    if LLM_CONFIG_PATHS
        .iter()
        .map(PathBuf::from)
        .any(|path| path.exists())
    {
        return Ok(());
    }

    ensure_llm_config_file(std::path::Path::new(LLM_CONFIG_PATHS[0]))
}

pub(crate) fn ensure_llm_config_file(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create llm config parent directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    let template = match ext.as_deref() {
        Some("toml") => {
            "[llm]\nenabled = false\nprovider = \"openai\"\nbase_url = \"https://tokenflux.dev/v1\"\nmodel = \"gpt-4.1-mini\"\ntimeout_seconds = 20\ncommand_prefix = \"/ask\"\napi_keys = []\n"
        }
        _ => {
            "llm:\n  enabled: false\n  provider: openai\n  base_url: https://tokenflux.dev/v1\n  model: gpt-4.1-mini\n  timeout_seconds: 20\n  command_prefix: /ask\n  api_keys: []\n"
        }
    };
    std::fs::write(path, template)
        .map_err(|err| format!("failed to write llm config {}: {err}", path.display()))?;
    Ok(())
}

pub(crate) fn load_app_config_with_llm_overlay() -> (AppConfigDoc, Vec<String>) {
    let (mut app_config, mut warnings) = load_app_config_with_warnings(false);
    if let Some(path) = resolve_llm_config_path() {
        match load_app_config_from_path(path.as_path()) {
            Ok(overlay_doc) => {
                if let Some(overlay_llm) = overlay_doc.llm {
                    app_config.llm = Some(merge_llm_config_sections(
                        app_config.llm.take(),
                        overlay_llm,
                    ));
                }
            }
            Err(err) => {
                let path_display = path.display().to_string();
                let err_text = err.to_string();
                warnings.push(
                    trf(
                        "startup.llm_overlay_load_failed",
                        &[("path", path_display.as_str()), ("err", err_text.as_str())],
                    )
                    .to_string(),
                );
            }
        }
    }
    warnings.extend(validate_app_config(&app_config));
    warnings = dedup_warnings(warnings);
    (app_config, warnings)
}

pub(crate) fn merge_llm_config_sections(
    base: Option<LlmConfigSection>,
    overlay: LlmConfigSection,
) -> LlmConfigSection {
    let mut merged = base.unwrap_or_default();
    if overlay.enabled.is_some() {
        merged.enabled = overlay.enabled;
    }
    if overlay.provider.is_some() {
        merged.provider = overlay.provider;
    }
    if overlay.base_url.is_some() {
        merged.base_url = overlay.base_url;
    }
    if overlay.provider_urls.is_some() {
        merged.provider_urls = overlay.provider_urls;
    }
    if overlay.api_keys.is_some() {
        merged.api_keys = overlay.api_keys;
    }
    if overlay.api_key.is_some() {
        merged.api_key = overlay.api_key;
    }
    if overlay.model.is_some() {
        merged.model = overlay.model;
    }
    if overlay.timeout_seconds.is_some() {
        merged.timeout_seconds = overlay.timeout_seconds;
    }
    if overlay.system_prompt.is_some() {
        merged.system_prompt = overlay.system_prompt;
    }
    if overlay.command_prefix.is_some() {
        merged.command_prefix = overlay.command_prefix;
    }
    merged
}

pub(crate) fn resolve_llm_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("LY_LLM_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return Some(PathBuf::from(path));
    }
    LLM_CONFIG_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

pub(crate) fn apply_runtime_log_overrides_from_app_config(
    runtime_config: &mut BotRuntimeConfig,
    app_config: &AppConfigDoc,
) {
    let runtime = app_config
        .rust
        .as_ref()
        .and_then(|section| section.runtime.as_ref())
        .or(app_config.runtime.as_ref());
    if let Some(runtime) = runtime {
        if let Some(worker_count) = runtime.worker_count
            && worker_count > 0
        {
            runtime_config.worker_count = worker_count;
        }
        if let Some(ingress_queue) = runtime.ingress_queue
            && ingress_queue > 0
        {
            runtime_config.ingress_queue = ingress_queue;
        }
        if let Some(worker_queue) = runtime.worker_queue
            && worker_queue > 0
        {
            runtime_config.worker_queue = worker_queue;
        }
    }

    let log = app_config
        .rust
        .as_ref()
        .and_then(|section| section.log.as_ref())
        .or(app_config.log.as_ref());
    if let Some(log) = log {
        if let Some(mode) = log.mode.as_deref()
            && let Some(mode) = LogMode::parse(mode)
        {
            runtime_config.logger.mode = mode;
        }
        if let Some(level) = log.level.as_deref()
            && let Some(level) = LogLevel::parse(level)
        {
            runtime_config.logger.min_level = level;
        }
        if let Some(timezone) = log.timezone.as_deref()
            && let Some(timezone) = TimeZone::parse(timezone)
        {
            runtime_config.logger.timezone = timezone;
        }
        if let Some(timestamp_format) = log.timestamp_format.as_deref() {
            if timestamp_format.trim().eq_ignore_ascii_case("custom") {
                let pattern = log
                    .timestamp_pattern
                    .as_deref()
                    .unwrap_or("%Y-%m-%d %H:%M:%S")
                    .to_string();
                runtime_config.logger.timestamp_format = TimestampFormat::Custom(pattern);
            } else {
                runtime_config.logger.timestamp_format = TimestampFormat::parse(timestamp_format);
            }
        } else if let Some(pattern) = log.timestamp_pattern.as_deref() {
            runtime_config.logger.timestamp_format = TimestampFormat::Custom(pattern.to_string());
        }
    }
}

pub(crate) fn describe_runtime_config(runtime_config: &BotRuntimeConfig) -> String {
    format!(
        "workers={}, ingress_queue={}, worker_queue={}, log_mode={}, log_level={}, log_tz={}, log_ts={}",
        runtime_config.worker_count,
        runtime_config.ingress_queue,
        runtime_config.worker_queue,
        runtime_config.logger.mode,
        runtime_config.logger.min_level,
        runtime_config.logger.timezone,
        runtime_config.logger.timestamp_format
    )
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
