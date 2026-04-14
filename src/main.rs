use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::Local;
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use liteyukibot_core::{
    AdapterConfig, LiteyukiBot, LogLevel, RuntimeSettings, RuntimeTarget,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;

const APP_TITLE: &str = "RsLiteyukiBot";
const DEFAULT_RUNTIME_TARGET: RuntimeTarget = RuntimeTarget::Cli;
const UI_LOG_CAPACITY: usize = 300;

#[derive(Debug, Clone, Copy)]
enum UiLevel {
    Info,
    Warn,
    Error,
    Event,
}

#[derive(Debug, Clone)]
struct UiLog {
    level: UiLevel,
    timestamp: String,
    message: String,
}

#[derive(Debug)]
enum UiEvent {
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
}

impl AppState {
    fn new(target: RuntimeTarget, settings_desc: String, adapters: Vec<AdapterConfig>) -> Self {
        let mut adapter_inbound_topics = HashSet::new();
        let mut adapter_running = HashMap::new();
        for adapter in &adapters {
            adapter_inbound_topics.insert(adapter.route.inbound_topic.clone());
            adapter_running.insert(adapter.id.clone(), false);
        }

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
                self.push_log(
                    UiLevel::Event,
                    format!("#{id} [{topic}] {payload_preview}"),
                );
            }
            UiEvent::Log { level, message } => {
                self.push_log(level, message);
            }
        }
    }

    fn refresh_adapter_state(&mut self, bot: &LiteyukiBot) {
        for adapter in &self.adapters {
            self.adapter_running
                .insert(adapter.id.clone(), bot.adapter_manager().is_running(&adapter.id));
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdapterConfigDoc {
    adapters: Vec<AdapterConfig>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = match RuntimeSettings::try_load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("failed to load runtime config from file/env, fallback to default: {err}");
            RuntimeSettings::default()
        }
    };
    let _ = settings.clone().install_global();
    let active_settings = RuntimeSettings::global_or_default();
    let mut runtime_config = RuntimeSettings::global_runtime_config().clone();
    runtime_config.logger.min_level = LogLevel::Error;

    let target = resolve_runtime_target();
    let adapter_configs = load_adapter_configs().unwrap_or_default();
    let adapter_autostart = !adapter_configs.is_empty();

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<UiEvent>();
    let ui_tx_for_handler = ui_tx.clone();

    let mut bot = LiteyukiBot::builder(APP_TITLE, env!("CARGO_PKG_VERSION"))
        .with_target(target)
        .with_runtime_config(runtime_config)
        .with_adapter_configs(adapter_configs.clone())
        .with_adapter_autostart(adapter_autostart)
        .with_event_handler(move |event, _logger| {
            let ui_tx_for_handler = ui_tx_for_handler.clone();
            async move {
                let _ = ui_tx_for_handler.send(UiEvent::RuntimeHandled {
                    id: event.id,
                    topic: event.topic.clone(),
                    payload_preview: payload_preview(&event.payload),
                });
            }
        })
        .build();

    let tx_before = ui_tx.clone();
    bot.on_before_start_sync(
        "tui-before-start",
        Default::default(),
        move |_context| {
            let _ = tx_before.send(UiEvent::Log {
                level: UiLevel::Info,
                message: "runtime preparing...".to_string(),
            });
            Ok(())
        },
    );

    let tx_after = ui_tx.clone();
    bot.on_after_start_sync("tui-after-start", Default::default(), move |_context| {
        let _ = tx_after.send(UiEvent::Log {
            level: UiLevel::Info,
            message: "runtime started".to_string(),
        });
        Ok(())
    });

    let tx_before_shutdown = ui_tx.clone();
    bot.on_before_process_shutdown_sync(
        "tui-before-shutdown",
        Default::default(),
        move |_context, process_name| {
            let _ = tx_before_shutdown.send(UiEvent::Log {
                level: UiLevel::Warn,
                message: format!("shutting down process: {}", process_name),
            });
            Ok(())
        },
    );

    bot.start().await?;

    let mut app = AppState::new(target, active_settings.describe(), adapter_configs);
    app.push_log(UiLevel::Info, "TUI ready: press q or Esc to quit");
    if adapter_autostart {
        app.push_log(UiLevel::Info, "adapters autostart enabled");
    }

    let mut terminal = init_terminal()?;
    let loop_result = run_tui_loop(&mut terminal, &mut bot, &mut app, &mut ui_rx).await;

    let _ = restore_terminal(&mut terminal);

    let shutdown_result = bot.shutdown().await;
    if let Err(err) = shutdown_result {
        eprintln!("bot shutdown failed: {err}");
    }

    loop_result?;
    Ok(())
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

    Ok(())
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, app: &AppState) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8), Constraint::Length(1)])
        .split(frame.area());

    let uptime = app.started_at.elapsed().as_secs();
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" RsLiteyukiBot ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw(format!(
            " target={:?}  uptime={}s  events={}  adapter_events={} ",
            app.target, uptime, app.total_events, app.adapter_events
        )),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Runtime"));
    frame.render_widget(header, root[0]);

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
    let adapter_list =
        List::new(adapter_items).block(Block::default().borders(Borders::ALL).title("Adapters"));
    frame.render_widget(adapter_list, mid[0]);

    let logs: Vec<ListItem<'_>> = app
        .logs
        .iter()
        .rev()
        .take((mid[1].height as usize).saturating_sub(2))
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
    let logs_widget = List::new(logs).block(Block::default().borders(Borders::ALL).title("Logs"));
    frame.render_widget(logs_widget, mid[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::raw("q / Esc to quit"),
        Span::raw("  |  "),
        Span::raw(format!("settings: {}", app.settings_desc)),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, root[2]);
}

fn poll_key_events(
    should_quit: &mut bool,
    app: &mut AppState,
) -> Result<(), Box<dyn std::error::Error>> {
    while event::poll(Duration::from_millis(0))? {
        if let CEvent::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    *should_quit = true;
                }
                KeyCode::Char('r') => {
                    app.push_log(UiLevel::Info, "refresh requested");
                }
                _ => {}
            }
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

fn resolve_runtime_target() -> RuntimeTarget {
    std::env::var("LY_RUNTIME_TARGET")
        .ok()
        .as_deref()
        .and_then(RuntimeTarget::parse)
        .unwrap_or(DEFAULT_RUNTIME_TARGET)
}

fn load_adapter_configs() -> Result<Vec<AdapterConfig>, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("LY_ADAPTERS_PATH") {
        let content = std::fs::read_to_string(PathBuf::from(path))?;
        if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(&content) {
            return Ok(doc.adapters);
        }
        let list = serde_json::from_str::<Vec<AdapterConfig>>(&content)?;
        return Ok(list);
    }

    if let Ok(raw) = std::env::var("LY_ADAPTERS_JSON") {
        if let Ok(doc) = serde_json::from_str::<AdapterConfigDoc>(&raw) {
            return Ok(doc.adapters);
        }
        let list = serde_json::from_str::<Vec<AdapterConfig>>(&raw)?;
        return Ok(list);
    }

    Ok(Vec::new())
}

fn payload_preview(payload: &Value) -> String {
    let raw = payload.to_string();
    const MAX: usize = 96;
    if raw.len() <= MAX {
        raw
    } else {
        format!("{}...", &raw[..MAX])
    }
}

