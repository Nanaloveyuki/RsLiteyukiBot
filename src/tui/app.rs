use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use chrono::Local;
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use liteyukibot_core::{AdapterConfig, AdapterTransport, LiteyukiBot, RuntimeTarget};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const UI_LOG_CAPACITY: usize = 300;
const COMMAND_HISTORY_CAPACITY: usize = 200;
const RESUME_FLUSH_INTERVAL: Duration = Duration::from_millis(800);
const DEFAULT_LOG_VIEW_ROWS: usize = 12;
const DEFAULT_RESUME_MAX_SESSIONS: usize = 64;
const DEFAULT_RESUME_MAX_SIZE_MIB: u64 = 16;
const TUI_COMMANDS: [&str; 13] = [
    "/help",
    "/reload",
    "/log",
    "/clear",
    "/adapters",
    "/ask",
    "/resumes",
    "/history",
    "/resume",
    "/llm",
    "/whitelist",
    "/quit",
    "/exit",
];
const LOG_SUBCOMMANDS: [&str; 2] = ["on", "off"];
const WHITELIST_SUBCOMMANDS: [&str; 3] = ["add", "remove", "list"];
const WHITELIST_SCOPE_HINTS: [&str; 4] = ["private", "group", "session", "user"];
const LLM_SUBCOMMANDS: [&str; 7] = [
    "model", "apikey", "provider", "enable", "disable", "on", "off",
];
const LLM_PROVIDER_HINTS: [&str; 1] = ["openai"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum UiLevel {
    Info,
    Warn,
    Error,
    Event,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UiLog {
    level: UiLevel,
    timestamp: String,
    message: String,
}

#[derive(Debug)]
pub enum UiEvent {
    RuntimeHandled {
        id: u64,
        topic: String,
        payload_preview: String,
    },
    ExternalStats {
        command_hits: u64,
        api_requests: u64,
        api_success: u64,
        api_failed: u64,
        api_timeouts: u64,
        api_inflight: u64,
    },
    Log {
        level: UiLevel,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiViewMode {
    Dashboard,
    LogConsole,
}

enum CommandOutcome {
    None,
    Quit,
    Reload,
    PersistWhitelist(Vec<String>),
    Llm(LlmCommandRequest),
    Ask(String),
}

enum AsyncCommandResult {
    Llm(Result<String, String>),
    Ask(Result<String, String>),
}

struct PollKeyEventsOutput {
    submitted_commands: Vec<String>,
    had_ui_change: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmCommandRequest {
    SetModel(String),
    AddApiKeys(Vec<String>),
    ProbeProvider(Option<String>),
    SetEnabled {
        enabled: bool,
        provider: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompletionMode {
    Command,
    ResumeUid,
    Rendered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionState {
    key: String,
    mode: CompletionMode,
    candidates: Vec<String>,
    index: usize,
}

#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub resume_store_path: PathBuf,
    pub resume_max_sessions: usize,
    pub resume_max_size_mib: u64,
}

#[derive(Debug, Clone)]
pub struct ReloadResult {
    pub adapters: Vec<AdapterConfig>,
    pub adapter_autostart: bool,
    pub tui_config: TuiConfig,
    pub help_whitelist: Vec<String>,
    pub warnings: Vec<String>,
}

pub type ReloadFuture<'a> = Pin<Box<dyn Future<Output = Result<ReloadResult, String>> + 'a>>;
pub type ReloadHandler = for<'a> fn(&'a mut LiteyukiBot) -> ReloadFuture<'a>;
pub type PersistWhitelistHandler = fn(Vec<String>) -> Result<String, String>;
pub type LlmCommandFuture<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;
pub type LlmCommandHandler = fn(LlmCommandRequest) -> LlmCommandFuture<'static>;
pub type AskFuture<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;
pub type AskHandler = fn(String) -> AskFuture<'static>;

pub struct RunOptions {
    pub target: RuntimeTarget,
    pub settings_desc: String,
    pub adapter_configs: Vec<AdapterConfig>,
    pub adapter_autostart: bool,
    pub tui_config: TuiConfig,
    pub reload_handler: ReloadHandler,
    pub whitelist_persist_handler: PersistWhitelistHandler,
    pub llm_command_handler: LlmCommandHandler,
    pub ask_handler: AskHandler,
    pub help_whitelist: Arc<RwLock<HashSet<String>>>,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            resume_store_path: PathBuf::from(".liteyuki-tui-resumes.json"),
            resume_max_sessions: DEFAULT_RESUME_MAX_SESSIONS,
            resume_max_size_mib: DEFAULT_RESUME_MAX_SIZE_MIB,
        }
    }
}

impl TuiConfig {
    fn normalized(self) -> Self {
        Self {
            resume_store_path: self.resume_store_path,
            resume_max_sessions: self.resume_max_sessions.max(1),
            resume_max_size_mib: self.resume_max_size_mib.max(1),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResumeSession {
    uid: String,
    created_at: String,
    updated_at: String,
    logs: Vec<UiLog>,
    command_history: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ResumeStore {
    sessions: Vec<ResumeSession>,
}

impl ResumeStore {
    fn load(path: &Path) -> Self {
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str::<ResumeStore>(&content).unwrap_or_default()
    }

    fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    fn create_session(&mut self, uid: String) {
        let now = Local::now().to_rfc3339();
        self.sessions.push(ResumeSession {
            uid,
            created_at: now.clone(),
            updated_at: now,
            logs: Vec::new(),
            command_history: Vec::new(),
        });
    }

    fn update_session(&mut self, uid: &str, logs: &VecDeque<UiLog>, command_history: &[String]) {
        let now = Local::now().to_rfc3339();
        let logs_vec: Vec<UiLog> = logs.iter().cloned().collect();
        let command_vec = command_history.to_vec();

        if let Some(pos) = self.sessions.iter().position(|session| session.uid == uid) {
            let mut session = self.sessions.remove(pos);
            session.updated_at = now;
            session.logs = logs_vec;
            session.command_history = command_vec;
            self.sessions.push(session);
        } else {
            self.sessions.push(ResumeSession {
                uid: uid.to_string(),
                created_at: now.clone(),
                updated_at: now,
                logs: logs_vec,
                command_history: command_vec,
            });
        }
    }

    fn get(&self, uid: &str) -> Option<&ResumeSession> {
        self.sessions.iter().find(|session| session.uid == uid)
    }

    fn estimated_size_bytes(&self) -> usize {
        serde_json::to_vec(self)
            .map(|bytes| bytes.len())
            .unwrap_or(usize::MAX)
    }
}

struct AppState {
    target: RuntimeTarget,
    settings_desc: String,
    started_at: Instant,
    logs: VecDeque<UiLog>,
    total_events: u64,
    adapter_events: u64,
    external_command_hits: u64,
    external_api_requests: u64,
    external_api_success: u64,
    external_api_failed: u64,
    external_api_timeouts: u64,
    external_api_inflight: u64,
    adapter_inbound_topics: HashSet<String>,
    adapters: Vec<AdapterConfig>,
    adapter_running: HashMap<String, bool>,
    console_input: String,
    command_history: Vec<String>,
    history_cursor: Option<usize>,
    history_draft: String,
    log_scroll: usize,
    log_view_rows: usize,
    resume_store_path: PathBuf,
    resume_store: ResumeStore,
    resume_max_sessions: usize,
    resume_max_size_bytes: usize,
    active_resume_uid: String,
    resume_dirty: bool,
    last_resume_flush: Instant,
    view_mode: UiViewMode,
    completion_state: Option<CompletionState>,
    help_whitelist: Option<Arc<RwLock<HashSet<String>>>>,
}

impl AppState {
    fn new(
        target: RuntimeTarget,
        settings_desc: String,
        adapters: Vec<AdapterConfig>,
        tui_config: TuiConfig,
    ) -> Self {
        let tui_config = tui_config.normalized();
        let mut adapter_inbound_topics = HashSet::new();
        let mut adapter_running = HashMap::new();
        for adapter in &adapters {
            adapter_inbound_topics.insert(adapter.route.inbound_topic.clone());
            adapter_running.insert(adapter.id.clone(), false);
        }
        let mut resume_store = ResumeStore::load(&tui_config.resume_store_path);
        let active_resume_uid = generate_resume_uid();
        resume_store.create_session(active_resume_uid.clone());
        let mut state = Self {
            target,
            settings_desc,
            started_at: Instant::now(),
            logs: VecDeque::with_capacity(UI_LOG_CAPACITY),
            total_events: 0,
            adapter_events: 0,
            external_command_hits: 0,
            external_api_requests: 0,
            external_api_success: 0,
            external_api_failed: 0,
            external_api_timeouts: 0,
            external_api_inflight: 0,
            adapter_inbound_topics,
            adapters,
            adapter_running,
            console_input: String::new(),
            command_history: Vec::new(),
            history_cursor: None,
            history_draft: String::new(),
            log_scroll: 0,
            log_view_rows: DEFAULT_LOG_VIEW_ROWS,
            resume_store_path: tui_config.resume_store_path,
            resume_store,
            resume_max_sessions: tui_config.resume_max_sessions,
            resume_max_size_bytes: mib_to_bytes(tui_config.resume_max_size_mib),
            active_resume_uid,
            resume_dirty: true,
            last_resume_flush: Instant::now(),
            view_mode: UiViewMode::Dashboard,
            completion_state: None,
            help_whitelist: None,
        };
        state.enforce_resume_limits();
        state
    }

    fn drop_oldest_non_active_resume(&mut self) -> bool {
        if self.resume_store.sessions.len() <= 1 {
            return false;
        }
        if let Some(index) = self
            .resume_store
            .sessions
            .iter()
            .position(|session| session.uid != self.active_resume_uid)
        {
            self.resume_store.sessions.remove(index);
            true
        } else {
            false
        }
    }

    fn enforce_resume_limits(&mut self) {
        let max_sessions = self.resume_max_sessions.max(1);
        while self.resume_store.sessions.len() > max_sessions {
            if !self.drop_oldest_non_active_resume() {
                break;
            }
        }

        let max_size_bytes = self.resume_max_size_bytes.max(1);
        while self.resume_store.estimated_size_bytes() > max_size_bytes {
            if !self.drop_oldest_non_active_resume() {
                break;
            }
        }
    }

    fn active_resume_uid(&self) -> &str {
        &self.active_resume_uid
    }

    fn apply_reload_result(&mut self, result: ReloadResult) {
        self.adapters = result.adapters;
        self.adapter_inbound_topics = self
            .adapters
            .iter()
            .map(|adapter| adapter.route.inbound_topic.clone())
            .collect();
        self.adapter_running = self
            .adapters
            .iter()
            .map(|adapter| (adapter.id.clone(), false))
            .collect();

        self.resume_store_path = result.tui_config.resume_store_path;
        self.resume_max_sessions = result.tui_config.resume_max_sessions.max(1);
        self.resume_max_size_bytes = mib_to_bytes(result.tui_config.resume_max_size_mib.max(1));
        self.enforce_resume_limits();
        self.resume_dirty = true;

        let mut whitelist_sync_failed = false;
        if let Some(shared) = self.help_whitelist.clone() {
            match shared.write() {
                Ok(mut lock) => {
                    lock.clear();
                    for entry in result.help_whitelist {
                        lock.insert(entry);
                    }
                }
                Err(_) => {
                    whitelist_sync_failed = true;
                }
            }
        }
        if whitelist_sync_failed {
            self.push_log(
                UiLevel::Error,
                "failed to sync whitelist from reloaded config",
            );
        }

        self.push_log(
            UiLevel::Info,
            format!(
                "reload applied: adapters={} autostart={} resume_max_sessions={} resume_max_size={} MiB",
                self.adapters.len(),
                result.adapter_autostart,
                self.resume_max_sessions,
                bytes_to_mib(self.resume_max_size_bytes),
            ),
        );
        for warning in result.warnings {
            self.push_log(UiLevel::Warn, format!("reload notice: {warning}"));
        }
    }

    fn push_log(&mut self, level: UiLevel, message: impl Into<String>) {
        let log = UiLog {
            level,
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            message: message.into(),
        };
        if self.logs.len() >= UI_LOG_CAPACITY {
            self.logs.pop_front();
        }
        self.logs.push_back(log);
        if self.log_scroll > 0 {
            self.log_scroll = self.log_scroll.saturating_add(1);
        }
        self.clamp_log_scroll();
        self.resume_dirty = true;
    }

    fn apply_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::RuntimeHandled {
                id,
                topic,
                payload_preview,
            } => {
                self.total_events += 1;
                if self.adapter_inbound_topics.contains(&topic) {
                    self.adapter_events += 1;
                }
                self.push_log(UiLevel::Event, format!("#{id} [{topic}] {payload_preview}"));
            }
            UiEvent::ExternalStats {
                command_hits,
                api_requests,
                api_success,
                api_failed,
                api_timeouts,
                api_inflight,
            } => {
                self.external_command_hits = command_hits;
                self.external_api_requests = api_requests;
                self.external_api_success = api_success;
                self.external_api_failed = api_failed;
                self.external_api_timeouts = api_timeouts;
                self.external_api_inflight = api_inflight;
            }
            UiEvent::Log { level, message } => {
                self.push_log(level, message);
            }
        }
    }

    fn refresh_adapter_state(&mut self, bot: &LiteyukiBot) -> bool {
        let mut state_changes = Vec::new();
        for adapter in &self.adapters {
            let next_running = bot.adapter_manager().is_running(&adapter.id);
            let prev_running = self
                .adapter_running
                .insert(adapter.id.clone(), next_running)
                .unwrap_or(false);
            if prev_running != next_running {
                state_changes.push((adapter.id.clone(), adapter.transport, next_running));
            }
        }

        let has_state_change = !state_changes.is_empty();
        for (id, transport, running) in state_changes {
            if running {
                self.push_log(
                    UiLevel::Info,
                    format!(
                        "adapter '{}' connected ({})",
                        id,
                        adapter_transport_label(transport)
                    ),
                );
            } else {
                self.push_log(UiLevel::Warn, format!("adapter '{}' disconnected", id));
            }
        }
        has_state_change
    }

    fn sync_active_resume_snapshot(&mut self) {
        self.resume_store.update_session(
            &self.active_resume_uid,
            &self.logs,
            &self.command_history,
        );
        self.enforce_resume_limits();
    }

    fn flush_resume_if_needed(&mut self, force: bool) {
        if !self.resume_dirty {
            return;
        }
        if !force && self.last_resume_flush.elapsed() < RESUME_FLUSH_INTERVAL {
            return;
        }

        self.sync_active_resume_snapshot();
        if let Err(err) = self.resume_store.save(&self.resume_store_path) {
            eprintln!(
                "failed to persist resume store to {}: {err}",
                self.resume_store_path.display()
            );
        } else {
            self.resume_dirty = false;
        }
        self.last_resume_flush = Instant::now();
    }

    fn reset_history_navigation(&mut self) {
        self.history_cursor = None;
        self.history_draft.clear();
        self.clear_completion_state();
    }

    fn detach_from_history_cursor(&mut self) {
        if self.history_cursor.is_some() {
            self.history_cursor = None;
            self.history_draft.clear();
        }
        self.clear_completion_state();
    }

    fn record_command(&mut self, command: &str) {
        if command.is_empty() {
            return;
        }
        if self
            .command_history
            .last()
            .is_some_and(|last| last.as_str() == command)
        {
            return;
        }
        if self.command_history.len() >= COMMAND_HISTORY_CAPACITY {
            self.command_history.remove(0);
        }
        self.command_history.push(command.to_string());
        self.reset_history_navigation();
        self.resume_dirty = true;
    }

    fn recall_previous_command(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        self.clear_completion_state();

        self.history_cursor = match self.history_cursor {
            None => {
                self.history_draft = self.console_input.clone();
                Some(self.command_history.len() - 1)
            }
            Some(0) => Some(0),
            Some(idx) => Some(idx - 1),
        };

        if let Some(idx) = self.history_cursor
            && let Some(command) = self.command_history.get(idx)
        {
            self.console_input = command.clone();
        }
    }

    fn recall_next_command(&mut self) {
        let Some(idx) = self.history_cursor else {
            return;
        };
        self.clear_completion_state();

        if idx + 1 < self.command_history.len() {
            self.history_cursor = Some(idx + 1);
            if let Some(command) = self.command_history.get(idx + 1) {
                self.console_input = command.clone();
            }
        } else {
            self.history_cursor = None;
            self.console_input = self.history_draft.clone();
            self.history_draft.clear();
        }
    }

    fn set_log_view_rows(&mut self, rows: usize) {
        self.log_view_rows = rows.max(1);
        self.clamp_log_scroll();
    }

    fn max_log_scroll(&self) -> usize {
        self.logs.len().saturating_sub(self.log_view_rows)
    }

    fn clamp_log_scroll(&mut self) {
        self.log_scroll = self.log_scroll.min(self.max_log_scroll());
    }

    fn scroll_logs_up(&mut self, lines: usize) {
        self.log_scroll = self
            .log_scroll
            .saturating_add(lines)
            .min(self.max_log_scroll());
    }

    fn scroll_logs_down(&mut self, lines: usize) {
        self.log_scroll = self.log_scroll.saturating_sub(lines);
    }

    fn scroll_logs_top(&mut self) {
        self.log_scroll = self.max_log_scroll();
    }

    fn scroll_logs_bottom(&mut self) {
        self.log_scroll = 0;
    }

    fn scroll_logs_page_up(&mut self) {
        let delta = self.log_view_rows.saturating_div(2).max(1);
        self.scroll_logs_up(delta);
    }

    fn scroll_logs_page_down(&mut self) {
        let delta = self.log_view_rows.saturating_div(2).max(1);
        self.scroll_logs_down(delta);
    }

    fn is_log_console_view(&self) -> bool {
        self.view_mode == UiViewMode::LogConsole
    }

    fn set_view_mode(&mut self, view_mode: UiViewMode) {
        self.view_mode = view_mode;
    }

    fn bind_help_whitelist(&mut self, whitelist: Arc<RwLock<HashSet<String>>>) {
        self.help_whitelist = Some(whitelist);
    }

    fn show_whitelist_usage(&mut self) {
        self.push_log(
            UiLevel::Warn,
            "usage: /whitelist list | /whitelist add <id|scope:id|scope id> | /whitelist remove <id|scope:id|scope id>",
        );
    }

    fn parse_whitelist_scope(scope: &str) -> Option<&'static str> {
        match scope.trim().to_ascii_lowercase().as_str() {
            "private" => Some("private"),
            "group" | "gourp" => Some("group"),
            "session" => Some("session"),
            "user" => Some("user"),
            _ => None,
        }
    }

    fn parse_whitelist_scoped_entry(scope: &str, id: &str) -> Result<String, String> {
        let Some(scope) = Self::parse_whitelist_scope(scope) else {
            return Err("scope should be private|group|session|user".to_string());
        };
        let id = id.trim();
        if id.is_empty() {
            return Err("id should not be empty".to_string());
        }
        Ok(format!("{scope}:{id}"))
    }

    fn parse_whitelist_entry(raw: &str) -> Result<String, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("entry should not be empty".to_string());
        }
        if let Some((scope, id)) = raw.split_once(':') {
            return Self::parse_whitelist_scoped_entry(scope, id);
        }
        Ok(raw.to_string())
    }

    fn with_whitelist_read<F>(&mut self, mut f: F)
    where
        F: FnMut(&HashSet<String>, &mut Self),
    {
        let Some(shared) = self.help_whitelist.clone() else {
            self.push_log(
                UiLevel::Warn,
                "whitelist bridge unavailable in current runtime",
            );
            return;
        };
        let Ok(lock) = shared.read() else {
            self.push_log(UiLevel::Error, "failed to lock whitelist (poisoned)");
            return;
        };
        f(&lock, self);
    }

    fn handle_whitelist_command(&mut self, args: &[&str]) -> CommandOutcome {
        let Some(subcommand) = args.first().copied() else {
            self.show_whitelist_usage();
            return CommandOutcome::None;
        };
        match subcommand {
            "list" => {
                if args.len() != 1 {
                    self.show_whitelist_usage();
                    return CommandOutcome::None;
                }
                self.with_whitelist_read(|set, app| {
                    if set.is_empty() {
                        app.push_log(
                            UiLevel::Info,
                            "external /help whitelist empty (allow all sessions)",
                        );
                        return;
                    }
                    let mut entries: Vec<String> = set.iter().cloned().collect();
                    entries.sort();
                    app.push_log(
                        UiLevel::Info,
                        format!("external /help whitelist entries ({}):", entries.len()),
                    );
                    for entry in entries {
                        app.push_log(UiLevel::Info, format!("  - {entry}"));
                    }
                });
                CommandOutcome::None
            }
            "add" | "remove" => {
                let entry = match args {
                    [_, value] => Self::parse_whitelist_entry(value),
                    [_, scope, id] => Self::parse_whitelist_scoped_entry(scope, id),
                    _ => {
                        self.show_whitelist_usage();
                        return CommandOutcome::None;
                    }
                };
                let Ok(entry) = entry else {
                    self.show_whitelist_usage();
                    return CommandOutcome::None;
                };
                let Some(shared) = self.help_whitelist.clone() else {
                    self.push_log(
                        UiLevel::Warn,
                        "whitelist bridge unavailable in current runtime",
                    );
                    return CommandOutcome::None;
                };
                let Ok(mut lock) = shared.write() else {
                    self.push_log(UiLevel::Error, "failed to lock whitelist (poisoned)");
                    return CommandOutcome::None;
                };

                if subcommand == "add" {
                    if lock.insert(entry.clone()) {
                        let mut entries: Vec<String> = lock.iter().cloned().collect();
                        entries.sort();
                        self.push_log(UiLevel::Info, format!("whitelist added: {entry}"));
                        self.push_log(UiLevel::Info, "persisting whitelist and auto reloading...");
                        return CommandOutcome::PersistWhitelist(entries);
                    }
                    self.push_log(UiLevel::Info, format!("whitelist already exists: {entry}"));
                    CommandOutcome::None
                } else {
                    if lock.remove(entry.as_str()) {
                        let mut entries: Vec<String> = lock.iter().cloned().collect();
                        entries.sort();
                        self.push_log(UiLevel::Info, format!("whitelist removed: {entry}"));
                        self.push_log(UiLevel::Info, "persisting whitelist and auto reloading...");
                        return CommandOutcome::PersistWhitelist(entries);
                    }
                    self.push_log(UiLevel::Info, format!("whitelist not found: {entry}"));
                    CommandOutcome::None
                }
            }
            _ => {
                self.show_whitelist_usage();
                CommandOutcome::None
            }
        }
    }

    fn show_llm_usage(&mut self) {
        self.push_log(
            UiLevel::Warn,
            "usage: /llm model <name> | /llm apikey <k1> [k2 ...] | /llm provider [name] | /llm on|off|enable|disable [provider]",
        );
    }

    fn parse_llm_provider(raw: &str) -> Option<String> {
        let provider = raw.trim().to_ascii_lowercase();
        if provider.is_empty() {
            None
        } else {
            Some(provider)
        }
    }

    fn parse_llm_api_keys(args: &[&str]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        for value in args {
            let key = value.trim().to_string();
            if key.is_empty() {
                continue;
            }
            if seen.insert(key.clone()) {
                keys.push(key);
            }
        }
        keys
    }

    fn handle_llm_command(&mut self, args: &[&str]) -> CommandOutcome {
        let Some(subcommand) = args.first().copied() else {
            self.show_llm_usage();
            return CommandOutcome::None;
        };

        match subcommand {
            "model" => {
                let model = match args {
                    [_, model] => model.trim(),
                    _ => {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    }
                };
                if model.is_empty() {
                    self.show_llm_usage();
                    return CommandOutcome::None;
                }
                self.push_log(UiLevel::Info, format!("updating llm.model -> {model}"));
                CommandOutcome::Llm(LlmCommandRequest::SetModel(model.to_string()))
            }
            "apikey" => {
                let keys = Self::parse_llm_api_keys(&args[1..]);
                if keys.is_empty() {
                    self.show_llm_usage();
                    return CommandOutcome::None;
                }
                self.push_log(
                    UiLevel::Info,
                    format!("adding {} api key(s) to llm.api_keys", keys.len()),
                );
                CommandOutcome::Llm(LlmCommandRequest::AddApiKeys(keys))
            }
            "provider" => {
                let provider = match args {
                    [_] => None,
                    [_, provider] => Self::parse_llm_provider(provider),
                    _ => {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    }
                };
                if let Some(provider) = provider.as_deref() {
                    self.push_log(
                        UiLevel::Info,
                        format!("probing llm provider (override={provider}) ..."),
                    );
                } else {
                    self.push_log(UiLevel::Info, "probing current llm provider ...");
                }
                CommandOutcome::Llm(LlmCommandRequest::ProbeProvider(provider))
            }
            "enable" | "on" | "disable" | "off" => {
                let enabled = matches!(subcommand, "enable" | "on");
                let provider = match args {
                    [_] => None,
                    [_, provider] => Self::parse_llm_provider(provider),
                    _ => {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    }
                };
                self.push_log(
                    UiLevel::Info,
                    format!(
                        "setting llm {}{}",
                        if enabled { "enabled" } else { "disabled" },
                        provider
                            .as_deref()
                            .map(|provider| format!(" (provider={provider})"))
                            .unwrap_or_default()
                    ),
                );
                CommandOutcome::Llm(LlmCommandRequest::SetEnabled { enabled, provider })
            }
            _ => {
                self.show_llm_usage();
                CommandOutcome::None
            }
        }
    }

    fn log_window_bounds(&self) -> (usize, usize) {
        let len = self.logs.len();
        if len == 0 {
            return (0, 0);
        }
        let rows = self.log_view_rows.max(1);
        let scroll = self.log_scroll.min(self.max_log_scroll());
        let start = len.saturating_sub(rows.saturating_add(scroll));
        let end = (start + rows).min(len);
        (start, end)
    }

    fn clear_completion_state(&mut self) {
        self.completion_state = None;
    }

    fn command_completion_candidates(&self, prefix: &str) -> Vec<String> {
        TUI_COMMANDS
            .iter()
            .filter(|command| command.starts_with(prefix))
            .map(|command| (*command).to_string())
            .collect()
    }

    fn log_completion_candidates(prefix: &str) -> Vec<String> {
        LOG_SUBCOMMANDS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| format!("/log {candidate}"))
            .collect()
    }

    fn whitelist_subcommand_candidates(prefix: &str) -> Vec<String> {
        WHITELIST_SUBCOMMANDS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| {
                if *candidate == "list" {
                    "/whitelist list".to_string()
                } else {
                    format!("/whitelist {candidate} ")
                }
            })
            .collect()
    }

    fn whitelist_scope_candidates(verb: &str, prefix: &str) -> Vec<String> {
        WHITELIST_SCOPE_HINTS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| format!("/whitelist {verb} {candidate} "))
            .collect()
    }

    fn llm_subcommand_candidates(prefix: &str) -> Vec<String> {
        LLM_SUBCOMMANDS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| match *candidate {
                "provider" => "/llm provider".to_string(),
                _ => format!("/llm {candidate} "),
            })
            .collect()
    }

    fn llm_provider_candidates(verb: &str, prefix: &str) -> Vec<String> {
        LLM_PROVIDER_HINTS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| format!("/llm {verb} {candidate}"))
            .collect()
    }

    fn resume_completion_candidates(&self, prefix: &str) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for uid in self
            .resume_store
            .sessions
            .iter()
            .rev()
            .map(|session| session.uid.as_str())
        {
            if uid.starts_with(prefix) && seen.insert(uid.to_string()) {
                candidates.push(uid.to_string());
            }
        }
        candidates
    }

    fn whitelist_completion_context(
        &self,
        input: &str,
    ) -> Option<(String, CompletionMode, Vec<String>)> {
        let rest = input.strip_prefix("/whitelist ")?;
        let rest = rest.trim_start();
        if rest.is_empty() {
            let candidates = Self::whitelist_subcommand_candidates("");
            return Some((
                "whitelist:subcommand:".to_string(),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let trailing_space = input.ends_with(' ');

        if tokens.len() == 1 {
            let verb = tokens[0];
            if trailing_space && matches!(verb, "add" | "remove") {
                let candidates = Self::whitelist_scope_candidates(verb, "");
                return Some((
                    format!("whitelist:{verb}:scope:"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            let candidates = Self::whitelist_subcommand_candidates(verb);
            return Some((
                format!("whitelist:subcommand:{verb}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        if tokens.len() == 2 && matches!(tokens[0], "add" | "remove") {
            let verb = tokens[0];
            let scope_prefix = if trailing_space { "" } else { tokens[1] };
            if scope_prefix.contains(':') {
                return None;
            }
            let candidates = Self::whitelist_scope_candidates(verb, scope_prefix);
            return Some((
                format!("whitelist:{verb}:scope:{scope_prefix}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        None
    }

    fn log_completion_context(&self, input: &str) -> Option<(String, CompletionMode, Vec<String>)> {
        let rest = input.strip_prefix("/log ")?;
        let rest = rest.trim_start();
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        if tokens.len() > 1 {
            return None;
        }
        let trailing_space = input.ends_with(' ');
        let prefix = if trailing_space {
            ""
        } else {
            tokens.first().copied().unwrap_or("")
        };
        let candidates = Self::log_completion_candidates(prefix);
        Some((
            format!("log:mode:{prefix}"),
            CompletionMode::Rendered,
            candidates,
        ))
    }

    fn llm_completion_context(&self, input: &str) -> Option<(String, CompletionMode, Vec<String>)> {
        let rest = input.strip_prefix("/llm ")?;
        let rest = rest.trim_start();
        if rest.is_empty() {
            let candidates = Self::llm_subcommand_candidates("");
            return Some((
                "llm:subcommand:".to_string(),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let trailing_space = input.ends_with(' ');
        if tokens.len() == 1 {
            let verb = tokens[0];
            if trailing_space && matches!(verb, "provider" | "enable" | "on" | "disable" | "off") {
                let candidates = Self::llm_provider_candidates(verb, "");
                return Some((
                    format!("llm:{verb}:provider:"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            let candidates = Self::llm_subcommand_candidates(verb);
            return Some((
                format!("llm:subcommand:{verb}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        if tokens.len() == 2
            && matches!(tokens[0], "provider" | "enable" | "on" | "disable" | "off")
        {
            let verb = tokens[0];
            let prefix = if trailing_space { "" } else { tokens[1] };
            let candidates = Self::llm_provider_candidates(verb, prefix);
            return Some((
                format!("llm:{verb}:provider:{prefix}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        None
    }

    fn completion_context(&self) -> Option<(String, CompletionMode, Vec<String>)> {
        let input = self.console_input.trim_start();
        if let Some(ctx) = self.whitelist_completion_context(input) {
            return Some(ctx);
        }
        if let Some(ctx) = self.log_completion_context(input) {
            return Some(ctx);
        }
        if let Some(ctx) = self.llm_completion_context(input) {
            return Some(ctx);
        }
        if let Some(prefix) = input.strip_prefix("/resume ") {
            let candidates = self.resume_completion_candidates(prefix);
            return Some((
                format!("resume:{prefix}"),
                CompletionMode::ResumeUid,
                candidates,
            ));
        }

        if !input.starts_with('/') {
            return None;
        }

        let candidates = self.command_completion_candidates(input);
        Some((
            format!("command:{input}"),
            CompletionMode::Command,
            candidates,
        ))
    }

    fn apply_completion_candidate(mode: CompletionMode, candidate: &str) -> String {
        match mode {
            CompletionMode::Command => {
                if matches!(candidate, "/resume" | "/whitelist" | "/llm" | "/ask") {
                    format!("{candidate} ")
                } else {
                    candidate.to_string()
                }
            }
            CompletionMode::ResumeUid => format!("/resume {candidate}"),
            CompletionMode::Rendered => candidate.to_string(),
        }
    }

    fn autocomplete_console_input(&mut self) {
        if let Some(state) = self.completion_state.clone() {
            if state.mode == CompletionMode::Command && state.candidates.len() == 1 {
                let rendered =
                    Self::apply_completion_candidate(state.mode.clone(), &state.candidates[0]);
                if self.console_input == rendered && rendered.ends_with(' ') {
                    self.clear_completion_state();
                }
            } else if let Some(current) = state.candidates.get(state.index) {
                let rendered = Self::apply_completion_candidate(state.mode.clone(), current);
                if self.console_input == rendered {
                    let next_index = (state.index + 1) % state.candidates.len();
                    if let Some(next) = state.candidates.get(next_index) {
                        self.completion_state = Some(CompletionState {
                            key: state.key,
                            mode: state.mode.clone(),
                            candidates: state.candidates.clone(),
                            index: next_index,
                        });
                        self.console_input = Self::apply_completion_candidate(state.mode, next);
                        self.history_cursor = None;
                        self.history_draft.clear();
                        return;
                    }
                } else {
                    self.clear_completion_state();
                }
            } else {
                self.clear_completion_state();
            }
        }

        let Some((key, mode, candidates)) = self.completion_context() else {
            self.clear_completion_state();
            return;
        };
        if candidates.is_empty() {
            self.clear_completion_state();
            return;
        }

        self.completion_state = Some(CompletionState {
            key,
            mode: mode.clone(),
            candidates: candidates.clone(),
            index: 0,
        });
        if let Some(first) = candidates.first() {
            self.console_input = Self::apply_completion_candidate(mode, first);
            self.history_cursor = None;
            self.history_draft.clear();
        }
    }

    fn completion_preview(&self) -> Option<String> {
        if !self.console_input.trim_start().starts_with('/') {
            return None;
        }
        let (_, mode, candidates) = self.completion_context()?;
        let candidate = candidates.first()?;
        Some(Self::apply_completion_candidate(mode, candidate))
    }

    fn completion_preview_suffix(&self) -> Option<String> {
        let preview = self.completion_preview()?;
        if preview == self.console_input {
            return None;
        }
        if let Some(suffix) = preview.strip_prefix(self.console_input.as_str()) {
            if suffix.is_empty() {
                None
            } else {
                Some(suffix.to_string())
            }
        } else {
            Some(format!("  ({preview})"))
        }
    }

    fn show_resume_list(&mut self) {
        if self.resume_store.sessions.is_empty() {
            self.push_log(UiLevel::Info, "no resume history found");
            return;
        }

        let mut lines: Vec<String> = self
            .resume_store
            .sessions
            .iter()
            .rev()
            .take(12)
            .map(|session| {
                let marker = if session.uid == self.active_resume_uid {
                    "*"
                } else {
                    " "
                };
                format!(
                    "{} {} (updated {}, logs {}, cmd {})",
                    marker,
                    session.uid,
                    session.updated_at,
                    session.logs.len(),
                    session.command_history.len()
                )
            })
            .collect();
        if self.resume_store.sessions.len() > lines.len() {
            lines.push(format!(
                "... and {} more",
                self.resume_store.sessions.len() - lines.len()
            ));
        }

        self.push_log(UiLevel::Info, "resume sessions (newest first, * active):");
        for line in lines {
            self.push_log(UiLevel::Info, line);
        }
    }

    fn switch_resume(&mut self, uid: &str) -> Result<(), String> {
        if uid == self.active_resume_uid {
            self.push_log(UiLevel::Info, format!("already in resume {uid}"));
            return Ok(());
        }

        self.sync_active_resume_snapshot();

        let session = self
            .resume_store
            .get(uid)
            .cloned()
            .ok_or_else(|| format!("resume not found: {uid}"))?;

        self.active_resume_uid = session.uid.clone();
        self.logs = session.logs.into_iter().collect();
        while self.logs.len() > UI_LOG_CAPACITY {
            self.logs.pop_front();
        }

        self.command_history = session.command_history;
        if self.command_history.len() > COMMAND_HISTORY_CAPACITY {
            let overflow = self.command_history.len() - COMMAND_HISTORY_CAPACITY;
            self.command_history.drain(0..overflow);
        }

        self.console_input.clear();
        self.reset_history_navigation();
        self.scroll_logs_bottom();
        self.push_log(
            UiLevel::Info,
            format!("resumed session {}", self.active_resume_uid),
        );
        self.resume_dirty = true;
        Ok(())
    }

    fn handle_console_command(&mut self, command: &str) -> CommandOutcome {
        let cmd = command.trim();
        let mut parts = cmd.split_whitespace();
        let command_name = parts.next().unwrap_or_default();
        match command_name {
            "/quit" | "/exit" => {
                self.push_log(UiLevel::Warn, "shutdown requested by console command");
                CommandOutcome::Quit
            }
            "/help" => {
                self.push_log(
                    UiLevel::Info,
                    "commands: /help /reload /log [on|off] /clear /adapters /ask <prompt> /resumes /history /resume <uid> /llm ... /whitelist ... /quit /exit",
                );
                self.push_log(
                    UiLevel::Info,
                    "whitelist: /whitelist list | /whitelist add <id|scope:id|scope id> | /whitelist remove <id|scope:id|scope id>",
                );
                self.push_log(
                    UiLevel::Info,
                    "llm: /llm model <name> | /llm apikey <k1> [k2 ...] | /llm provider [name] | /llm on|off|enable|disable [provider]",
                );
                self.push_log(
                    UiLevel::Info,
                    "keys: Up/Down history, Tab cycle-complete (e.g. / and /h), PgUp/PgDn/Home/End scroll logs",
                );
                self.push_log(
                    UiLevel::Info,
                    "in /log view: empty input + Up/Down scroll logs by line",
                );
                self.push_log(
                    UiLevel::Info,
                    format!("active resume: {}", self.active_resume_uid),
                );
                CommandOutcome::None
            }
            "/reload" => {
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, "usage: /reload");
                    return CommandOutcome::None;
                }
                self.push_log(UiLevel::Info, "reload requested");
                CommandOutcome::Reload
            }
            "/log" => {
                let Some(mode_arg) = parts.next() else {
                    let next_mode = if self.is_log_console_view() {
                        UiViewMode::Dashboard
                    } else {
                        UiViewMode::LogConsole
                    };
                    self.set_view_mode(next_mode);
                    let label = if self.is_log_console_view() {
                        "entered /log view (full log + command console)"
                    } else {
                        "returned to dashboard view"
                    };
                    self.push_log(UiLevel::Info, label);
                    return CommandOutcome::None;
                };
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, "usage: /log [on|off]");
                    return CommandOutcome::None;
                }
                match mode_arg {
                    "on" => {
                        self.set_view_mode(UiViewMode::LogConsole);
                        self.push_log(
                            UiLevel::Info,
                            "entered /log view (full log + command console)",
                        );
                    }
                    "off" => {
                        self.set_view_mode(UiViewMode::Dashboard);
                        self.push_log(UiLevel::Info, "returned to dashboard view");
                    }
                    _ => {
                        self.push_log(UiLevel::Warn, "usage: /log [on|off]");
                    }
                }
                CommandOutcome::None
            }
            "/clear" => {
                self.logs.clear();
                self.scroll_logs_bottom();
                self.push_log(UiLevel::Info, "console cleared");
                CommandOutcome::None
            }
            "/adapters" => {
                if self.adapters.is_empty() {
                    self.push_log(UiLevel::Info, "no adapters configured");
                    return CommandOutcome::None;
                }
                let lines: Vec<String> = self
                    .adapters
                    .iter()
                    .map(|adapter| {
                        let status = if self
                            .adapter_running
                            .get(&adapter.id)
                            .copied()
                            .unwrap_or(false)
                        {
                            "RUN"
                        } else {
                            "IDLE"
                        };
                        format!(
                            "{} [{}] {:?} -> {}",
                            adapter.id, status, adapter.transport, adapter.endpoint.url
                        )
                    })
                    .collect();
                for line in lines {
                    self.push_log(UiLevel::Info, line);
                }
                CommandOutcome::None
            }
            "/ask" => {
                let prompt = parts.collect::<Vec<&str>>().join(" ").trim().to_string();
                if prompt.is_empty() {
                    self.push_log(UiLevel::Warn, "usage: /ask <prompt>");
                    return CommandOutcome::None;
                }
                self.push_log(
                    UiLevel::Info,
                    "sending /ask request in background (ui remains responsive)...",
                );
                CommandOutcome::Ask(prompt)
            }
            "/resumes" | "/history" => {
                self.show_resume_list();
                CommandOutcome::None
            }
            "/resume" => {
                let Some(uid) = parts.next() else {
                    self.push_log(
                        UiLevel::Info,
                        format!(
                            "current resume: {}. usage: /resume <uid> (list by /resumes)",
                            self.active_resume_uid
                        ),
                    );
                    return CommandOutcome::None;
                };
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, "usage: /resume <uid>");
                    return CommandOutcome::None;
                }
                if let Err(err) = self.switch_resume(uid) {
                    self.push_log(UiLevel::Warn, err);
                }
                CommandOutcome::None
            }
            "/whitelist" => {
                let args: Vec<&str> = parts.collect();
                self.handle_whitelist_command(&args)
            }
            "/llm" => {
                let args: Vec<&str> = parts.collect();
                self.handle_llm_command(&args)
            }
            _ => {
                self.push_log(UiLevel::Warn, format!("unknown command: {cmd}. try /help"));
                CommandOutcome::None
            }
        }
    }

    fn command_help_text(&self) -> String {
        let input = self.console_input.trim();
        if input.is_empty() {
            return "命令说明: 输入 /help 查看命令；Tab 自动补全；Enter 执行".to_string();
        }

        if !input.starts_with('/') {
            return "命令说明: 普通文本不会执行命令，请以 / 开头；例如 /ask 你好".to_string();
        }

        if let Some(help) = Self::command_help_for_line(input) {
            return help.to_string();
        }

        if let Some((_, mode, candidates)) = self.completion_context()
            && let Some(candidate) = candidates.first()
        {
            let rendered = Self::apply_completion_candidate(mode, candidate);
            if let Some(help) = Self::command_help_for_line(rendered.as_str()) {
                return help.to_string();
            }
        }

        "命令说明: 未知命令，输入 /help 查看可用命令".to_string()
    }

    fn command_help_for_line(line: &str) -> Option<&'static str> {
        let mut parts = line.split_whitespace();
        let command = parts.next()?;

        match command {
            "/help" => Some("命令说明: /help 显示全部命令与快捷键"),
            "/reload" => Some("命令说明: /reload 重新加载配置与适配器状态"),
            "/log" => Some("命令说明: /log [on|off] 切换日志控制台视图"),
            "/clear" => Some("命令说明: /clear 清空当前日志窗口"),
            "/adapters" => Some("命令说明: /adapters 列出适配器连接状态与端点"),
            "/ask" => Some("命令说明: /ask <prompt> 后台请求 LLM，不阻塞终端刷新"),
            "/resumes" | "/history" => Some("命令说明: /resumes 或 /history 查看历史会话快照"),
            "/resume" => Some("命令说明: /resume <uid> 切换到指定历史会话"),
            "/llm" => Some("命令说明: /llm 管理模型、Key、provider 与开关"),
            "/whitelist" => Some("命令说明: /whitelist 管理 external /help 白名单"),
            "/quit" | "/exit" => Some("命令说明: /quit 或 /exit 安全退出程序"),
            _ => None,
        }
    }
}

pub async fn run(
    bot: &mut LiteyukiBot,
    options: RunOptions,
    ui_rx: &mut mpsc::UnboundedReceiver<UiEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let RunOptions {
        target,
        settings_desc,
        adapter_configs,
        adapter_autostart,
        tui_config,
        reload_handler,
        whitelist_persist_handler,
        llm_command_handler,
        ask_handler,
        help_whitelist,
    } = options;
    let mut app = AppState::new(target, settings_desc, adapter_configs, tui_config);
    app.bind_help_whitelist(help_whitelist);
    app.push_log(
        UiLevel::Info,
        "TUI ready: type /help in console, Ctrl+C or /quit to exit",
    );
    app.push_log(
        UiLevel::Info,
        format!("new resume created: {}", app.active_resume_uid()),
    );
    app.push_log(
        UiLevel::Info,
        format!(
            "resume policy: max_sessions={}, max_size={} MiB",
            app.resume_max_sessions,
            bytes_to_mib(app.resume_max_size_bytes)
        ),
    );
    if adapter_autostart {
        app.push_log(UiLevel::Info, "adapters autostart enabled");
    }

    let mut terminal = init_terminal()?;
    let loop_result = run_tui_loop(
        &mut terminal,
        bot,
        &mut app,
        reload_handler,
        whitelist_persist_handler,
        llm_command_handler,
        ask_handler,
        ui_rx,
    )
    .await;

    let _ = restore_terminal(&mut terminal);
    app.flush_resume_if_needed(true);
    loop_result
}

async fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    bot: &mut LiteyukiBot,
    app: &mut AppState,
    reload_handler: ReloadHandler,
    whitelist_persist_handler: PersistWhitelistHandler,
    llm_command_handler: LlmCommandHandler,
    ask_handler: AskHandler,
    ui_rx: &mut mpsc::UnboundedReceiver<UiEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tick = tokio::time::interval(Duration::from_millis(120));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut should_quit = false;
    let mut needs_redraw = true;
    let mut last_uptime_sec = app.started_at.elapsed().as_secs();
    let (async_result_tx, mut async_result_rx) = mpsc::unbounded_channel::<AsyncCommandResult>();

    while !should_quit {
        tokio::select! {
            _ = tick.tick() => {
                let mut ui_changed = false;
                let uptime_sec = app.started_at.elapsed().as_secs();
                if uptime_sec != last_uptime_sec {
                    last_uptime_sec = uptime_sec;
                    ui_changed = true;
                }

                if app.refresh_adapter_state(bot) {
                    ui_changed = true;
                }

                let poll_output = poll_key_events(&mut should_quit, app)?;
                ui_changed |= poll_output.had_ui_change;
                for command in poll_output.submitted_commands {
                    match app.handle_console_command(&command) {
                        CommandOutcome::Quit => {
                            should_quit = true;
                            ui_changed = true;
                        }
                        CommandOutcome::Reload => {
                            app.push_log(UiLevel::Info, "reloading config...");
                            ui_changed = true;
                            match reload_handler(bot).await {
                                Ok(result) => {
                                    app.apply_reload_result(result);
                                    if app.refresh_adapter_state(bot) {
                                        ui_changed = true;
                                    }
                                }
                                Err(err) => {
                                    app.push_log(UiLevel::Warn, format!("reload failed: {err}"));
                                    ui_changed = true;
                                }
                            }
                        }
                        CommandOutcome::PersistWhitelist(entries) => {
                            match whitelist_persist_handler(entries) {
                                Ok(message) => {
                                    app.push_log(UiLevel::Info, message);
                                    app.push_log(UiLevel::Info, "reloading config...");
                                    ui_changed = true;
                                    match reload_handler(bot).await {
                                        Ok(result) => {
                                            app.apply_reload_result(result);
                                            if app.refresh_adapter_state(bot) {
                                                ui_changed = true;
                                            }
                                        }
                                        Err(err) => {
                                            app.push_log(
                                                UiLevel::Warn,
                                                format!("reload failed: {err}"),
                                            );
                                            ui_changed = true;
                                        }
                                    }
                                }
                                Err(err) => {
                                    app.push_log(UiLevel::Warn, format!("persist whitelist failed: {err}"));
                                    app.push_log(
                                        UiLevel::Warn,
                                        "runtime whitelist changed but config was not persisted",
                                    );
                                    ui_changed = true;
                                }
                            }
                        }
                        CommandOutcome::Llm(request) => {
                            let tx = async_result_tx.clone();
                            tokio::spawn(async move {
                                let result = llm_command_handler(request).await;
                                let _ = tx.send(AsyncCommandResult::Llm(result));
                            });
                            ui_changed = true;
                        }
                        CommandOutcome::Ask(prompt) => {
                            let tx = async_result_tx.clone();
                            tokio::spawn(async move {
                                let result = ask_handler(prompt).await;
                                let _ = tx.send(AsyncCommandResult::Ask(result));
                            });
                            ui_changed = true;
                        }
                        CommandOutcome::None => {
                            ui_changed = true;
                        }
                    }
                }
                needs_redraw |= ui_changed;
            }
            Some(event) = ui_rx.recv() => {
                app.apply_event(event);
                while let Ok(next) = ui_rx.try_recv() {
                    app.apply_event(next);
                }
                needs_redraw = true;
            }
            Some(async_result) = async_result_rx.recv() => {
                match async_result {
                    AsyncCommandResult::Llm(result) => match result {
                        Ok(message) => app.push_log(UiLevel::Info, message),
                        Err(err) => app.push_log(UiLevel::Warn, format!("llm command failed: {err}")),
                    },
                    AsyncCommandResult::Ask(result) => match result {
                        Ok(message) => app.push_log(UiLevel::Info, message),
                        Err(err) => app.push_log(UiLevel::Warn, format!("ask failed: {err}")),
                    },
                }
                needs_redraw = true;
            }
            signal = tokio::signal::ctrl_c() => {
                if signal.is_ok() {
                    app.push_log(UiLevel::Warn, "Ctrl+C received");
                } else {
                    app.push_log(UiLevel::Error, "failed to listen Ctrl+C signal");
                }
                should_quit = true;
                needs_redraw = true;
            }
        }

        app.flush_resume_if_needed(false);
        if needs_redraw {
            terminal.draw(|frame| draw_ui(frame, app))?;
            needs_redraw = false;
        }
    }

    app.flush_resume_if_needed(true);
    Ok(())
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, app: &mut AppState) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let uptime = app.started_at.elapsed().as_secs();
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " RsLiteyukiBot ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(format!(
            " target={:?}  uptime={}s  events={}  adapter_events={}  ext_cmd={}  api_inflight={}  resume={} ",
            app.target,
            uptime,
            app.total_events,
            app.adapter_events,
            app.external_command_hits,
            app.external_api_inflight,
            app.active_resume_uid
        )),
    ]))
    .block(rounded_block("Runtime"));
    frame.render_widget(header, root[0]);

    if app.is_log_console_view() {
        draw_log_console_view(frame, app, root[1], root[2]);
        return;
    }

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(24), Constraint::Percentage(76)])
        .split(root[1]);

    let adapter_text_width = (mid[0].width as usize).saturating_sub(2).max(1);
    let adapter_items: Vec<ListItem<'_>> = if app.adapters.is_empty() {
        vec![ListItem::new(Line::from("no adapters configured"))]
    } else {
        app.adapters
            .iter()
            .map(|adapter| {
                let running = app
                    .adapter_running
                    .get(&adapter.id)
                    .copied()
                    .unwrap_or(false);
                let status = if running { "RUN" } else { "IDLE" };
                let status_style = if running {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let name_style = if running {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let mut lines = vec![Line::from(vec![
                    Span::styled(format!("[{}] ", status), status_style),
                    Span::styled(adapter.id.clone(), name_style),
                ])];
                let detail = format!(
                    "({}) {}",
                    adapter_transport_label(adapter.transport),
                    adapter.endpoint.url
                );
                let detail_lines = wrap_text_hard(&detail, adapter_text_width.saturating_sub(2));
                for detail_line in detail_lines {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(detail_line, Style::default().fg(Color::DarkGray)),
                    ]));
                }
                ListItem::new(Text::from(lines))
            })
            .collect()
    };
    let adapter_list = List::new(adapter_items).block(rounded_block("Adapters"));
    frame.render_widget(adapter_list, mid[0]);

    let console = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(3),
            Constraint::Length(4),
        ])
        .split(mid[1]);
    render_external_panel(frame, app, console[0]);
    render_logs_panel(frame, app, console[1], "Console");
    render_command_panel(frame, app, console[2]);

    let footer = Paragraph::new(Line::from(vec![
        Span::raw("/help"),
        Span::raw("  |  "),
        Span::raw("Up/Down history, Tab cycle-complete, PgUp/PgDn/Home/End scroll"),
        Span::raw("  |  "),
        Span::raw("Ctrl+C to quit"),
        Span::raw("  |  "),
        Span::raw(format!("settings: {}", app.settings_desc)),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, root[2]);
}

fn draw_log_console_view(
    frame: &mut ratatui::Frame<'_>,
    app: &mut AppState,
    content_area: Rect,
    footer_area: Rect,
) {
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(4)])
        .split(content_area);
    render_logs_panel(frame, app, content[0], "Log");
    render_command_panel(frame, app, content[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::raw("/help for commands"),
        Span::raw("  |  "),
        Span::raw("Empty input + Up/Down or PgUp/PgDn/Home/End scroll logs"),
        Span::raw("  |  "),
        Span::raw("Ctrl+Up/Down history, Tab cycle-complete"),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn render_external_panel(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let success_rate = if app.external_api_requests == 0 {
        0.0
    } else {
        (app.external_api_success as f64) / (app.external_api_requests as f64) * 100.0
    };

    let line1 = Line::from(vec![
        Span::styled("commands=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.external_command_hits.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("inflight=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.external_api_inflight.to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let line2 = Line::from(vec![
        Span::styled("api req=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.external_api_requests.to_string(),
            Style::default().fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled("ok=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.external_api_success.to_string(),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled("fail=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.external_api_failed.to_string(),
            Style::default().fg(Color::Red),
        ),
        Span::raw("  "),
        Span::styled("timeout=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.external_api_timeouts.to_string(),
            Style::default().fg(Color::Magenta),
        ),
        Span::raw("  "),
        Span::styled("rate=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{success_rate:.1}%"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let widget = Paragraph::new(Text::from(vec![line1, line2]))
        .block(rounded_block("External EventHandle"))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn render_logs_panel(frame: &mut ratatui::Frame<'_>, app: &mut AppState, area: Rect, title: &str) {
    let log_rows = (area.height as usize).saturating_sub(2).max(1);
    let log_text_width = (area.width as usize).saturating_sub(2).max(1);
    app.set_log_view_rows(log_rows);
    let max_scroll = app.max_log_scroll();
    let (start, end) = app.log_window_bounds();

    let logs: Vec<ListItem<'_>> = app
        .logs
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|log| {
            let (tag, style) = match log.level {
                UiLevel::Info => ("INFO", Style::default().fg(Color::Blue)),
                UiLevel::Warn => ("WARN", Style::default().fg(Color::Yellow)),
                UiLevel::Error => ("ERR ", Style::default().fg(Color::Red)),
                UiLevel::Event => ("EVT ", Style::default().fg(Color::Green)),
            };
            let prefix = format!("{} [{}] ", log.timestamp, tag);
            let prefix_width = UnicodeWidthStr::width(prefix.as_str());
            let message_width = log_text_width.saturating_sub(prefix_width).max(1);
            let wrapped_message = wrap_text_hard(log.message.as_str(), message_width);
            let first_line = wrapped_message.first().cloned().unwrap_or_default();
            let mut lines = vec![Line::from(vec![
                Span::styled(
                    format!("{} ", log.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("[{}] ", tag), style),
                Span::raw(first_line),
            ])];
            let indent = " ".repeat(prefix_width);
            for segment in wrapped_message.iter().skip(1) {
                lines.push(Line::from(vec![
                    Span::raw(indent.clone()),
                    Span::raw(segment.clone()),
                ]));
            }
            ListItem::new(Text::from(lines))
        })
        .collect();

    let panel_title = if app.log_scroll > 0 {
        format!("{title} (scroll {}/{max_scroll})", app.log_scroll)
    } else {
        title.to_string()
    };
    let logs_widget = List::new(logs).block(rounded_block(panel_title.as_str()));
    frame.render_widget(logs_widget, area);
}

fn render_command_panel(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(area);

    render_command_help_bar(frame, app, chunks[0]);

    let mut spans = vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(app.console_input.as_str()),
    ];
    if let Some(preview) = app.completion_preview_suffix() {
        spans.push(Span::styled(
            preview,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ));
    }

    let input_area = chunks[1];
    let input_widget = Paragraph::new(Line::from(spans)).block(rounded_block("Command"));
    frame.render_widget(input_widget, input_area);
    let (cursor_x, cursor_y) = command_cursor_position(input_area, app.console_input.as_str());
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn render_command_help_bar(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let style = Style::default()
        .fg(Color::Black)
        .bg(Color::White)
        .add_modifier(Modifier::BOLD);
    let widget = Paragraph::new(Line::from(vec![Span::raw(app.command_help_text())]))
        .style(style)
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn command_cursor_position(area: Rect, input: &str) -> (u16, u16) {
    let inner_x = area.x.saturating_add(1);
    let inner_y = area.y.saturating_add(1);
    let available_width = area.width.saturating_sub(2) as usize;
    let prompt_width = UnicodeWidthStr::width("> ");
    let input_width = UnicodeWidthStr::width(input);
    let max_offset = available_width.saturating_sub(1);
    let offset = (prompt_width + input_width).min(max_offset) as u16;
    (inner_x.saturating_add(offset), inner_y)
}

fn adapter_transport_label(transport: AdapterTransport) -> &'static str {
    match transport {
        AdapterTransport::WebSocketForward => "ws-forward",
        AdapterTransport::WebSocketReverse => "ws-reverse",
        AdapterTransport::Sse => "sse",
        AdapterTransport::Http => "http",
    }
}

fn wrap_text_hard(text: &str, max_width: usize) -> Vec<String> {
    let width = max_width.max(1);
    let mut lines = Vec::new();

    for source_line in text.lines() {
        if source_line.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut chunk = String::new();
        let mut chunk_width = 0usize;
        for ch in source_line.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if !chunk.is_empty() && chunk_width + ch_width > width {
                lines.push(chunk);
                chunk = String::new();
                chunk_width = 0;
            }
            chunk.push(ch);
            chunk_width += ch_width;
            if chunk_width >= width {
                lines.push(chunk);
                chunk = String::new();
                chunk_width = 0;
            }
        }
        if !chunk.is_empty() {
            lines.push(chunk);
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn poll_key_events(
    should_quit: &mut bool,
    app: &mut AppState,
) -> Result<PollKeyEventsOutput, Box<dyn std::error::Error>> {
    let mut submitted = Vec::new();
    let mut had_ui_change = false;
    while event::poll(Duration::from_millis(0))? {
        let CEvent::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            app.push_log(UiLevel::Warn, "Ctrl+C key event received");
            *should_quit = true;
            had_ui_change = true;
            continue;
        }

        match key.code {
            KeyCode::Enter => {
                had_ui_change = true;
                let input = app.console_input.trim().to_string();
                if !input.is_empty() {
                    app.push_log(UiLevel::Info, format!("> {}", input));
                    app.record_command(&input);
                    submitted.push(input);
                }
                app.console_input.clear();
                app.reset_history_navigation();
            }
            KeyCode::Backspace => {
                had_ui_change = true;
                app.detach_from_history_cursor();
                app.console_input.pop();
            }
            KeyCode::Up => {
                had_ui_change = true;
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.recall_previous_command();
                } else if app.is_log_console_view()
                    && app.console_input.is_empty()
                    && app.history_cursor.is_none()
                {
                    app.clear_completion_state();
                    app.scroll_logs_up(1);
                } else {
                    app.recall_previous_command();
                }
            }
            KeyCode::Down => {
                had_ui_change = true;
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.recall_next_command();
                } else if app.is_log_console_view()
                    && app.console_input.is_empty()
                    && app.history_cursor.is_none()
                {
                    app.clear_completion_state();
                    app.scroll_logs_down(1);
                } else {
                    app.recall_next_command();
                }
            }
            KeyCode::Tab => {
                had_ui_change = true;
                app.autocomplete_console_input();
            }
            KeyCode::PageUp => {
                had_ui_change = true;
                app.clear_completion_state();
                app.scroll_logs_page_up();
            }
            KeyCode::PageDown => {
                had_ui_change = true;
                app.clear_completion_state();
                app.scroll_logs_page_down();
            }
            KeyCode::Home => {
                had_ui_change = true;
                app.clear_completion_state();
                app.scroll_logs_top();
            }
            KeyCode::End => {
                had_ui_change = true;
                app.clear_completion_state();
                app.scroll_logs_bottom();
            }
            KeyCode::Char(ch) => {
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    continue;
                }
                had_ui_change = true;
                app.detach_from_history_cursor();
                app.console_input.push(ch);
            }
            KeyCode::Esc => {
                had_ui_change = true;
                app.console_input.clear();
                app.reset_history_navigation();
            }
            _ => {}
        }
    }
    Ok(PollKeyEventsOutput {
        submitted_commands: submitted,
        had_ui_change,
    })
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn rounded_block<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .title(title)
}

fn generate_resume_uid() -> String {
    format!(
        "resume-{}-{}",
        Local::now().format("%Y%m%d%H%M%S%6f"),
        std::process::id()
    )
}

fn mib_to_bytes(mib: u64) -> usize {
    let bytes = (mib as u128) * 1024 * 1024;
    bytes.min(usize::MAX as u128) as usize
}

fn bytes_to_mib(bytes: usize) -> usize {
    let mib = bytes / (1024 * 1024);
    mib.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_resume_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!("rsliteyuki-{name}-{nanos}.json"));
        path
    }

    fn remove_file_if_exists(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    fn test_tui_config(path: PathBuf) -> TuiConfig {
        TuiConfig {
            resume_store_path: path,
            resume_max_sessions: 64,
            resume_max_size_mib: 16,
        }
    }

    #[test]
    fn command_history_navigation_restores_draft() {
        let path = temp_resume_path("history-navigation");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );
        app.record_command("/help");
        app.record_command("/clear");

        app.console_input = "/res".to_string();
        app.recall_previous_command();
        assert_eq!(app.console_input, "/clear");

        app.recall_previous_command();
        assert_eq!(app.console_input, "/help");

        app.recall_next_command();
        assert_eq!(app.console_input, "/clear");

        app.recall_next_command();
        assert_eq!(app.console_input, "/res");

        remove_file_if_exists(&path);
    }

    #[test]
    fn autocomplete_completes_resume_uid() {
        let path = temp_resume_path("autocomplete");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );
        app.resume_store
            .create_session("resume-alpha-001".to_string());

        app.console_input = "/resume resume-alpha".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/resume resume-alpha-001");

        app.console_input = "/he".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/help");

        remove_file_if_exists(&path);
    }

    #[test]
    fn completion_preview_shows_suffix_for_best_match() {
        let path = temp_resume_path("completion-preview");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        app.console_input = "/re".to_string();
        assert_eq!(app.completion_preview_suffix().as_deref(), Some("load"));

        app.console_input = "/help".to_string();
        assert!(app.completion_preview_suffix().is_none());

        app.resume_store
            .create_session("resume-preview-001".to_string());
        app.console_input = "/resume resume-pr".to_string();
        assert_eq!(
            app.completion_preview_suffix().as_deref(),
            Some("eview-001")
        );

        remove_file_if_exists(&path);
    }

    #[test]
    fn autocomplete_cycles_slash_commands() {
        let path = temp_resume_path("autocomplete-cycle-slash");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        app.console_input = "/".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/help");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/reload");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/log");

        remove_file_if_exists(&path);
    }

    #[test]
    fn autocomplete_cycles_h_prefix_between_help_and_history() {
        let path = temp_resume_path("autocomplete-cycle-h");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        app.console_input = "/h".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/help");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/history");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/help");

        remove_file_if_exists(&path);
    }

    #[test]
    fn autocomplete_whitelist_subcommands_and_scope() {
        let path = temp_resume_path("autocomplete-whitelist");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        app.console_input = "/whitelist ".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/whitelist add ");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/whitelist remove ");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/whitelist list");

        app.console_input = "/whitelist add ".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/whitelist add private ");

        remove_file_if_exists(&path);
    }

    #[test]
    fn autocomplete_log_subcommands() {
        let path = temp_resume_path("autocomplete-log");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        app.console_input = "/log ".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/log on");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/log off");

        remove_file_if_exists(&path);
    }

    #[test]
    fn command_cursor_position_tracks_input_width() {
        let area = Rect::new(0, 0, 20, 3);
        let (x, y) = command_cursor_position(area, "/help");
        assert_eq!(y, 1);
        assert_eq!(x, 1 + 2 + 5);

        let (x_cjk, y_cjk) = command_cursor_position(area, "你好");
        assert_eq!(y_cjk, 1);
        assert_eq!(x_cjk, 1 + 2 + 4);
    }

    #[test]
    fn wrap_text_hard_respects_display_width_for_cjk() {
        let wrapped = wrap_text_hard("你好世界", 4);
        assert_eq!(wrapped, vec!["你好".to_string(), "世界".to_string()]);

        let wrapped_mixed = wrap_text_hard("ab你好cd", 4);
        assert_eq!(wrapped_mixed, vec!["ab你".to_string(), "好cd".to_string()]);
    }

    #[test]
    fn command_help_text_shows_ask_non_blocking_hint() {
        let path = temp_resume_path("command-help-ask");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );
        app.console_input = "/ask hello".to_string();
        let help = app.command_help_text();
        assert!(help.contains("/ask <prompt>"));
        assert!(help.contains("不阻塞终端刷新"));

        remove_file_if_exists(&path);
    }

    #[test]
    fn command_help_text_uses_completion_for_prefix() {
        let path = temp_resume_path("command-help-prefix");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );
        app.console_input = "/as".to_string();
        let help = app.command_help_text();
        assert!(help.contains("/ask <prompt>"));

        remove_file_if_exists(&path);
    }

    #[test]
    fn switch_resume_restores_previous_snapshot() {
        let path = temp_resume_path("switch");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );
        let original_uid = app.active_resume_uid().to_string();
        app.push_log(UiLevel::Info, "from-original");
        app.record_command("/help");

        let now = Local::now().to_rfc3339();
        let restore_uid = "resume-restore-001".to_string();
        app.resume_store.sessions.push(ResumeSession {
            uid: restore_uid.clone(),
            created_at: now.clone(),
            updated_at: now,
            logs: vec![UiLog {
                level: UiLevel::Info,
                timestamp: "00:00:00".to_string(),
                message: "from-restore".to_string(),
            }],
            command_history: vec!["/clear".to_string()],
        });

        app.switch_resume(&restore_uid)
            .expect("resume switch should work");
        assert_eq!(app.active_resume_uid(), restore_uid.as_str());
        assert!(app.logs.iter().any(|log| log.message == "from-restore"));
        assert!(
            app.command_history
                .iter()
                .any(|command| command.as_str() == "/clear")
        );

        let original_snapshot = app
            .resume_store
            .get(&original_uid)
            .expect("original resume should be saved");
        assert!(
            original_snapshot
                .logs
                .iter()
                .any(|log| log.message == "from-original")
        );

        remove_file_if_exists(&path);
    }

    #[test]
    fn log_window_keeps_chronological_order_with_scroll() {
        let path = temp_resume_path("log-window");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );
        for i in 1..=5 {
            app.push_log(UiLevel::Info, format!("line-{i}"));
        }

        app.set_log_view_rows(3);
        let (start, end) = app.log_window_bounds();
        let lines: Vec<String> = app
            .logs
            .iter()
            .skip(start)
            .take(end - start)
            .map(|log| log.message.clone())
            .collect();
        assert_eq!(lines, vec!["line-3", "line-4", "line-5"]);

        app.scroll_logs_up(1);
        let (start, end) = app.log_window_bounds();
        let lines: Vec<String> = app
            .logs
            .iter()
            .skip(start)
            .take(end - start)
            .map(|log| log.message.clone())
            .collect();
        assert_eq!(lines, vec!["line-2", "line-3", "line-4"]);

        remove_file_if_exists(&path);
    }

    #[test]
    fn log_command_switches_view_mode() {
        let path = temp_resume_path("log-view-mode");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        assert!(!app.is_log_console_view());
        app.handle_console_command("/log");
        assert!(app.is_log_console_view());
        app.handle_console_command("/log off");
        assert!(!app.is_log_console_view());
        app.handle_console_command("/log on");
        assert!(app.is_log_console_view());

        remove_file_if_exists(&path);
    }

    #[test]
    fn reload_command_is_recognized() {
        let path = temp_resume_path("reload-command");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        let outcome = app.handle_console_command("/reload");
        assert!(matches!(outcome, CommandOutcome::Reload));

        let outcome = app.handle_console_command("/reload now");
        assert!(matches!(outcome, CommandOutcome::None));
        assert!(
            app.logs
                .iter()
                .any(|log| log.message.contains("usage: /reload"))
        );

        remove_file_if_exists(&path);
    }

    #[test]
    fn whitelist_command_can_add_remove_and_list_entries() {
        let path = temp_resume_path("whitelist-command");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );
        let shared = Arc::new(RwLock::new(HashSet::new()));
        app.bind_help_whitelist(shared.clone());

        let outcome = app.handle_console_command("/whitelist add private 3541766758");
        assert!(matches!(outcome, CommandOutcome::PersistWhitelist(_)));
        let outcome = app.handle_console_command("/whitelist add gourp:699493240");
        assert!(matches!(outcome, CommandOutcome::PersistWhitelist(_)));
        app.handle_console_command("/whitelist list");

        let lock = shared
            .read()
            .expect("whitelist lock should be readable in test");
        assert!(lock.contains("private:3541766758"));
        assert!(lock.contains("group:699493240"));
        drop(lock);

        let outcome = app.handle_console_command("/whitelist remove group:699493240");
        assert!(matches!(outcome, CommandOutcome::PersistWhitelist(_)));
        let lock = shared
            .read()
            .expect("whitelist lock should be readable in test");
        assert!(!lock.contains("group:699493240"));
        assert!(lock.contains("private:3541766758"));
        drop(lock);

        assert!(
            app.logs
                .iter()
                .any(|log| log.message.contains("whitelist entries"))
        );

        remove_file_if_exists(&path);
    }

    #[test]
    fn llm_command_parses_actions() {
        let path = temp_resume_path("llm-command");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        let outcome = app.handle_console_command("/llm model gpt-4.1-mini");
        assert!(matches!(
            outcome,
            CommandOutcome::Llm(LlmCommandRequest::SetModel(_))
        ));

        let outcome = app.handle_console_command("/llm apikey k1 k2");
        assert!(matches!(
            outcome,
            CommandOutcome::Llm(LlmCommandRequest::AddApiKeys(_))
        ));

        let outcome = app.handle_console_command("/llm provider openai");
        assert!(matches!(
            outcome,
            CommandOutcome::Llm(LlmCommandRequest::ProbeProvider(Some(_)))
        ));

        let outcome = app.handle_console_command("/llm on openai");
        assert!(matches!(
            outcome,
            CommandOutcome::Llm(LlmCommandRequest::SetEnabled { enabled: true, .. })
        ));

        remove_file_if_exists(&path);
    }

    #[test]
    fn ask_command_parses_prompt() {
        let path = temp_resume_path("ask-command");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        let outcome = app.handle_console_command("/ask hello world");
        assert!(matches!(outcome, CommandOutcome::Ask(_)));

        let outcome = app.handle_console_command("/ask");
        assert!(matches!(outcome, CommandOutcome::None));
        assert!(
            app.logs
                .iter()
                .any(|log| log.message.contains("usage: /ask"))
        );

        remove_file_if_exists(&path);
    }

    #[test]
    fn autocomplete_includes_ask_command() {
        let path = temp_resume_path("autocomplete-ask");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        app.console_input = "/as".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/ask ");

        remove_file_if_exists(&path);
    }

    #[test]
    fn autocomplete_llm_subcommands_and_provider() {
        let path = temp_resume_path("autocomplete-llm");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            test_tui_config(path.clone()),
        );

        app.console_input = "/llm ".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/llm model ");

        app.console_input = "/llm on ".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/llm on openai");

        remove_file_if_exists(&path);
    }

    #[test]
    fn resume_count_limit_keeps_active_session() {
        let path = temp_resume_path("resume-count-limit");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            TuiConfig {
                resume_store_path: path.clone(),
                resume_max_sessions: 2,
                resume_max_size_mib: 16,
            },
        );
        let active_uid = app.active_resume_uid().to_string();

        app.resume_store.create_session("old-1".to_string());
        app.resume_store.create_session("old-2".to_string());
        app.resume_store.create_session("old-3".to_string());
        app.enforce_resume_limits();

        assert!(app.resume_store.sessions.len() <= 2);
        assert!(
            app.resume_store
                .sessions
                .iter()
                .any(|session| session.uid == active_uid)
        );

        remove_file_if_exists(&path);
    }

    #[test]
    fn resume_size_limit_keeps_active_session() {
        let path = temp_resume_path("resume-size-limit");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            TuiConfig {
                resume_store_path: path.clone(),
                resume_max_sessions: 64,
                resume_max_size_mib: 1,
            },
        );
        let active_uid = app.active_resume_uid().to_string();

        let large = "x".repeat(600 * 1024);
        app.resume_store.sessions.push(ResumeSession {
            uid: "old-large-a".to_string(),
            created_at: Local::now().to_rfc3339(),
            updated_at: Local::now().to_rfc3339(),
            logs: vec![UiLog {
                level: UiLevel::Info,
                timestamp: "00:00:00".to_string(),
                message: large.clone(),
            }],
            command_history: vec![],
        });
        app.resume_store.sessions.push(ResumeSession {
            uid: "old-large-b".to_string(),
            created_at: Local::now().to_rfc3339(),
            updated_at: Local::now().to_rfc3339(),
            logs: vec![UiLog {
                level: UiLevel::Info,
                timestamp: "00:00:00".to_string(),
                message: large,
            }],
            command_history: vec![],
        });

        app.enforce_resume_limits();

        assert!(app.resume_store.estimated_size_bytes() <= app.resume_max_size_bytes);
        assert!(
            app.resume_store
                .sessions
                .iter()
                .any(|session| session.uid == active_uid)
        );

        remove_file_if_exists(&path);
    }
}
