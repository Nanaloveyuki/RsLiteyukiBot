use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::Local;
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use liteyukibot_core::{AdapterConfig, LiteyukiBot, RuntimeTarget};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

const UI_LOG_CAPACITY: usize = 300;
const COMMAND_HISTORY_CAPACITY: usize = 200;
const RESUME_STORE_CAPACITY: usize = 64;
const RESUME_FLUSH_INTERVAL: Duration = Duration::from_millis(800);
const DEFAULT_LOG_VIEW_ROWS: usize = 12;
const TUI_COMMANDS: [&str; 9] = [
    "/help",
    "/log",
    "/clear",
    "/adapters",
    "/resumes",
    "/history",
    "/resume",
    "/quit",
    "/exit",
];

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompletionMode {
    Command,
    ResumeUid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionState {
    key: String,
    mode: CompletionMode,
    candidates: Vec<String>,
    index: usize,
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
        self.trim();
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

        self.trim();
    }

    fn get(&self, uid: &str) -> Option<&ResumeSession> {
        self.sessions.iter().find(|session| session.uid == uid)
    }

    fn trim(&mut self) {
        if self.sessions.len() <= RESUME_STORE_CAPACITY {
            return;
        }
        let overflow = self.sessions.len() - RESUME_STORE_CAPACITY;
        self.sessions.drain(0..overflow);
    }
}

struct AppState {
    target: RuntimeTarget,
    settings_desc: String,
    started_at: Instant,
    logs: VecDeque<UiLog>,
    total_events: u64,
    adapter_events: u64,
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
    active_resume_uid: String,
    resume_dirty: bool,
    last_resume_flush: Instant,
    view_mode: UiViewMode,
    completion_state: Option<CompletionState>,
}

impl AppState {
    fn new(
        target: RuntimeTarget,
        settings_desc: String,
        adapters: Vec<AdapterConfig>,
        resume_store_path: PathBuf,
    ) -> Self {
        let mut adapter_inbound_topics = HashSet::new();
        let mut adapter_running = HashMap::new();
        for adapter in &adapters {
            adapter_inbound_topics.insert(adapter.route.inbound_topic.clone());
            adapter_running.insert(adapter.id.clone(), false);
        }
        let mut resume_store = ResumeStore::load(&resume_store_path);
        let active_resume_uid = generate_resume_uid();
        resume_store.create_session(active_resume_uid.clone());

        Self {
            target,
            settings_desc,
            started_at: Instant::now(),
            logs: VecDeque::with_capacity(UI_LOG_CAPACITY),
            total_events: 0,
            adapter_events: 0,
            adapter_inbound_topics,
            adapters,
            adapter_running,
            console_input: String::new(),
            command_history: Vec::new(),
            history_cursor: None,
            history_draft: String::new(),
            log_scroll: 0,
            log_view_rows: DEFAULT_LOG_VIEW_ROWS,
            resume_store_path,
            resume_store,
            active_resume_uid,
            resume_dirty: true,
            last_resume_flush: Instant::now(),
            view_mode: UiViewMode::Dashboard,
            completion_state: None,
        }
    }

    fn active_resume_uid(&self) -> &str {
        &self.active_resume_uid
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
            UiEvent::Log { level, message } => {
                self.push_log(level, message);
            }
        }
    }

    fn refresh_adapter_state(&mut self, bot: &LiteyukiBot) {
        for adapter in &self.adapters {
            self.adapter_running.insert(
                adapter.id.clone(),
                bot.adapter_manager().is_running(&adapter.id),
            );
        }
    }

    fn sync_active_resume_snapshot(&mut self) {
        self.resume_store.update_session(
            &self.active_resume_uid,
            &self.logs,
            &self.command_history,
        );
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

    fn completion_context(&self) -> Option<(String, CompletionMode, Vec<String>)> {
        let input = self.console_input.trim_start();
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
                if candidate == "/resume" {
                    format!("{candidate} ")
                } else {
                    candidate.to_string()
                }
            }
            CompletionMode::ResumeUid => format!("/resume {candidate}"),
        }
    }

    fn autocomplete_console_input(&mut self) {
        if let Some(state) = self.completion_state.clone() {
            if state.mode == CompletionMode::Command
                && state.candidates.len() == 1
                && state.candidates[0].as_str() == "/resume"
                && self.console_input.trim_start() == "/resume "
            {
                self.clear_completion_state();
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

    fn handle_console_command(&mut self, command: &str) -> bool {
        let cmd = command.trim();
        let mut parts = cmd.split_whitespace();
        let command_name = parts.next().unwrap_or_default();
        match command_name {
            "/quit" | "/exit" => {
                self.push_log(UiLevel::Warn, "shutdown requested by console command");
                true
            }
            "/help" => {
                self.push_log(
                    UiLevel::Info,
                    "commands: /help /log [on|off] /clear /adapters /resumes /history /resume <uid> /quit /exit",
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
                false
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
                    return false;
                };
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, "usage: /log [on|off]");
                    return false;
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
                false
            }
            "/clear" => {
                self.logs.clear();
                self.scroll_logs_bottom();
                self.push_log(UiLevel::Info, "console cleared");
                false
            }
            "/adapters" => {
                if self.adapters.is_empty() {
                    self.push_log(UiLevel::Info, "no adapters configured");
                    return false;
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
                false
            }
            "/resumes" | "/history" => {
                self.show_resume_list();
                false
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
                    return false;
                };
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, "usage: /resume <uid>");
                    return false;
                }
                if let Err(err) = self.switch_resume(uid) {
                    self.push_log(UiLevel::Warn, err);
                }
                false
            }
            _ => {
                self.push_log(UiLevel::Warn, format!("unknown command: {cmd}. try /help"));
                false
            }
        }
    }
}

pub async fn run(
    bot: &mut LiteyukiBot,
    target: RuntimeTarget,
    settings_desc: String,
    adapter_configs: Vec<AdapterConfig>,
    adapter_autostart: bool,
    ui_rx: &mut mpsc::UnboundedReceiver<UiEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let resume_store_path = resolve_resume_store_path();
    let mut app = AppState::new(target, settings_desc, adapter_configs, resume_store_path);
    app.push_log(
        UiLevel::Info,
        "TUI ready: type /help in console, Ctrl+C or /quit to exit",
    );
    app.push_log(
        UiLevel::Info,
        format!("new resume created: {}", app.active_resume_uid()),
    );
    if adapter_autostart {
        app.push_log(UiLevel::Info, "adapters autostart enabled");
    }

    let mut terminal = init_terminal()?;
    let loop_result = run_tui_loop(&mut terminal, bot, &mut app, ui_rx).await;

    let _ = restore_terminal(&mut terminal);
    app.flush_resume_if_needed(true);
    loop_result
}

async fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    bot: &mut LiteyukiBot,
    app: &mut AppState,
    ui_rx: &mut mpsc::UnboundedReceiver<UiEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tick = tokio::time::interval(Duration::from_millis(120));
    let mut should_quit = false;

    while !should_quit {
        tokio::select! {
            _ = tick.tick() => {
                app.refresh_adapter_state(bot);
                poll_key_events(&mut should_quit, app)?;
                app.flush_resume_if_needed(false);
                terminal.draw(|frame| draw_ui(frame, app))?;
            }
            Some(event) = ui_rx.recv() => {
                app.apply_event(event);
            }
            signal = tokio::signal::ctrl_c() => {
                if signal.is_ok() {
                    app.push_log(UiLevel::Warn, "Ctrl+C received");
                } else {
                    app.push_log(UiLevel::Error, "failed to listen Ctrl+C signal");
                }
                should_quit = true;
            }
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
            " target={:?}  uptime={}s  events={}  adapter_events={}  resume={} ",
            app.target, uptime, app.total_events, app.adapter_events, app.active_resume_uid
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
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(root[1]);

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
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", status), status_style),
                    Span::raw(format!("{} ({:?})", adapter.id, adapter.transport)),
                ]))
            })
            .collect()
    };
    let adapter_list = List::new(adapter_items).block(rounded_block("Adapters"));
    frame.render_widget(adapter_list, mid[0]);

    let console = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(mid[1]);
    render_logs_panel(frame, app, console[0], "Console");
    render_command_panel(frame, app, console[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::raw("/help /log /clear /adapters /resumes /resume <uid> /quit"),
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
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(content_area);
    render_logs_panel(frame, app, content[0], "Log");
    render_command_panel(frame, app, content[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::raw("/log off to return dashboard"),
        Span::raw("  |  "),
        Span::raw("Empty input + Up/Down or PgUp/PgDn/Home/End scroll logs"),
        Span::raw("  |  "),
        Span::raw("Ctrl+Up/Down history, Tab cycle-complete"),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn render_logs_panel(frame: &mut ratatui::Frame<'_>, app: &mut AppState, area: Rect, title: &str) {
    let log_rows = (area.height as usize).saturating_sub(2).max(1);
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
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", log.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("[{}] ", tag), style),
                Span::raw(log.message.clone()),
            ]))
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
    let input_widget = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(app.console_input.as_str()),
    ]))
    .block(rounded_block("Command"));
    frame.render_widget(input_widget, area);
}

fn poll_key_events(
    should_quit: &mut bool,
    app: &mut AppState,
) -> Result<(), Box<dyn std::error::Error>> {
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
            continue;
        }

        match key.code {
            KeyCode::Enter => {
                let input = app.console_input.trim().to_string();
                if !input.is_empty() {
                    app.push_log(UiLevel::Info, format!("> {}", input));
                    let should_exit = app.handle_console_command(&input);
                    app.record_command(&input);
                    if should_exit {
                        *should_quit = true;
                    }
                }
                app.console_input.clear();
                app.reset_history_navigation();
            }
            KeyCode::Backspace => {
                app.detach_from_history_cursor();
                app.console_input.pop();
            }
            KeyCode::Up => {
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
                app.autocomplete_console_input();
            }
            KeyCode::PageUp => {
                app.clear_completion_state();
                app.scroll_logs_page_up();
            }
            KeyCode::PageDown => {
                app.clear_completion_state();
                app.scroll_logs_page_down();
            }
            KeyCode::Home => {
                app.clear_completion_state();
                app.scroll_logs_top();
            }
            KeyCode::End => {
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
                app.detach_from_history_cursor();
                app.console_input.push(ch);
            }
            KeyCode::Esc => {
                app.console_input.clear();
                app.reset_history_navigation();
            }
            _ => {}
        }
    }
    Ok(())
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

fn resolve_resume_store_path() -> PathBuf {
    std::env::var("LY_RESUME_STORE_PATH")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".liteyuki-tui-resumes.json"))
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

    #[test]
    fn command_history_navigation_restores_draft() {
        let path = temp_resume_path("history-navigation");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            path.clone(),
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
            path.clone(),
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
    fn autocomplete_cycles_slash_commands() {
        let path = temp_resume_path("autocomplete-cycle-slash");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            path.clone(),
        );

        app.console_input = "/".to_string();
        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/help");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/log");

        app.autocomplete_console_input();
        assert_eq!(app.console_input, "/clear");

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
            path.clone(),
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
    fn switch_resume_restores_previous_snapshot() {
        let path = temp_resume_path("switch");
        remove_file_if_exists(&path);

        let mut app = AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            path.clone(),
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
            path.clone(),
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
            path.clone(),
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
}
