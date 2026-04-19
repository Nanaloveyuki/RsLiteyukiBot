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
use liteyukibot_core::observability::set_console_log_output_enabled;
use liteyukibot_core::{AdapterConfig, AdapterTransport, LiteyukiBot, PluginSdk, RuntimeTarget};
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

mod commands;
mod input;
mod render;
mod runtime;
mod state;

use self::input::poll_key_events;
#[cfg(test)]
use self::render::command_cursor_position;
use self::render::draw_ui;
pub use self::runtime::run;

const UI_LOG_CAPACITY: usize = 300;
const COMMAND_HISTORY_CAPACITY: usize = 200;
const RESUME_FLUSH_INTERVAL: Duration = Duration::from_millis(800);
const ASYNC_COMMAND_RESULT_CAPACITY: usize = 64;
const ASYNC_COMMAND_CONCURRENCY_LIMIT: usize = 4;
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
const LLM_SUBCOMMANDS: [&str; 8] = [
    "model", "apikey", "provider", "enable", "disable", "on", "off", "prompt",
];
const LLM_PROVIDER_HINTS: [&str; 1] = ["openai"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiLevel {
    Info,
    Warn,
    Error,
    Event,
    Llm,
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
    PluginCommand { command: String, args: Vec<String> },
}

enum AsyncCommandResult {
    Llm(Result<String, String>),
    Ask(Result<String, String>),
    PluginCommand(Result<String, String>),
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
    AddProviderUrl(String),
    RemoveProviderUrl(String),
    ListProviderUrls,
    UseProviderUrl(String),
    SetEnabled {
        enabled: bool,
        provider: Option<String>,
    },
    PromptList,
    PromptUse(String),
    PromptSet {
        name: String,
        soul: String,
    },
    PromptRemove(String),
    PromptPreview {
        user_prompt: String,
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
    pub llm_command_prefix: String,
    pub warnings: Vec<String>,
}

pub type ReloadFuture<'a> = Pin<Box<dyn Future<Output = Result<ReloadResult, String>> + 'a>>;
pub type ReloadHandler = for<'a> fn(&'a LiteyukiBot) -> ReloadFuture<'a>;
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
    pub llm_command_prefix: Arc<RwLock<String>>,
    pub plugin_sdk: PluginSdk,
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
    log_text_width: usize,
    resume_store_path: PathBuf,
    resume_store: ResumeStore,
    resume_max_sessions: usize,
    resume_max_size_bytes: usize,
    active_resume_uid: String,
    resume_dirty: bool,
    last_resume_flush: Instant,
    last_resume_save_error: Option<String>,
    view_mode: UiViewMode,
    completion_state: Option<CompletionState>,
    help_whitelist: Option<Arc<RwLock<HashSet<String>>>>,
    llm_command_prefix: Option<Arc<RwLock<String>>>,
    plugin_sdk: Option<PluginSdk>,
}

fn adapter_transport_label(transport: AdapterTransport) -> &'static str {
    match transport {
        AdapterTransport::WebSocketForward => "ws-forward",
        AdapterTransport::WebSocketReverse => "ws-reverse",
        AdapterTransport::Sse => "sse",
        AdapterTransport::Http => "http",
    }
}

fn log_level_tag(level: UiLevel) -> &'static str {
    match level {
        UiLevel::Info => "INFO",
        UiLevel::Warn => "WARN",
        UiLevel::Error => "ERR ",
        UiLevel::Event => "EVT ",
        UiLevel::Llm => "LLM ",
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

fn init_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(err) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(err.into());
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(err) => {
            let _ = disable_raw_mode();
            let mut rollback_stdout = io::stdout();
            let _ = execute!(rollback_stdout, LeaveAlternateScreen);
            return Err(err.into());
        }
    };
    if let Err(err) = terminal.clear() {
        let _ = restore_terminal(&mut terminal);
        return Err(err.into());
    }
    Ok(terminal)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut errors = Vec::new();
    if let Err(err) = disable_raw_mode() {
        errors.push(format!("disable_raw_mode failed: {err}"));
    }
    if let Err(err) = execute!(terminal.backend_mut(), LeaveAlternateScreen) {
        errors.push(format!("leave alternate screen failed: {err}"));
    }
    if let Err(err) = terminal.show_cursor() {
        errors.push(format!("show cursor failed: {err}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(errors.join("; ")).into())
    }
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
mod tests;
