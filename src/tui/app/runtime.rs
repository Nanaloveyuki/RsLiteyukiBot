use super::*;
use tokio::sync::Semaphore;

#[derive(Clone, Copy)]
struct LoopHandlers {
    reload_handler: ReloadHandler,
    whitelist_persist_handler: PersistWhitelistHandler,
    llm_command_handler: LlmCommandHandler,
    ask_handler: AskHandler,
}

pub async fn run(
    bot: &LiteyukiBot,
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
    let handlers = LoopHandlers {
        reload_handler,
        whitelist_persist_handler,
        llm_command_handler,
        ask_handler,
    };
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

    let previous_console_log_output = set_console_log_output_enabled(false);
    let mut terminal = match init_terminal() {
        Ok(terminal) => terminal,
        Err(err) => {
            set_console_log_output_enabled(previous_console_log_output);
            return Err(err);
        }
    };
    let loop_result = run_tui_loop(&mut terminal, bot, &mut app, handlers, ui_rx).await;
    let restore_result = restore_terminal(&mut terminal);
    set_console_log_output_enabled(previous_console_log_output);
    app.flush_resume_if_needed(true);
    match (loop_result, restore_result) {
        (Err(err), _) => Err(err),
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(err)) => Err(err),
    }
}

async fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    bot: &LiteyukiBot,
    app: &mut AppState,
    handlers: LoopHandlers,
    ui_rx: &mut mpsc::UnboundedReceiver<UiEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tick = tokio::time::interval(Duration::from_millis(120));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut should_quit = false;
    let mut needs_redraw = true;
    let mut last_uptime_sec = app.started_at.elapsed().as_secs();
    let (async_result_tx, mut async_result_rx) =
        mpsc::channel::<AsyncCommandResult>(ASYNC_COMMAND_RESULT_CAPACITY);
    let async_command_limiter = Arc::new(Semaphore::new(ASYNC_COMMAND_CONCURRENCY_LIMIT));
    let mut reload_inflight: Option<ReloadFuture<'_>> = None;
    let mut queued_reload = false;

    while !should_quit {
        tokio::select! {
            reload_result = async {
                match reload_inflight.as_mut() {
                    Some(future) => Some(future.await),
                    None => None,
                }
            }, if reload_inflight.is_some() => {
                if let Some(result) = reload_result {
                    match result {
                        Ok(result) => {
                            app.apply_reload_result(result);
                            app.refresh_adapter_state(bot);
                        }
                        Err(err) => {
                            app.push_log(UiLevel::Warn, format!("reload failed: {err}"));
                        }
                    }
                }
                reload_inflight = None;
                if queued_reload {
                    queued_reload = false;
                    app.push_log(UiLevel::Info, "processing queued reload request...");
                    app.push_log(UiLevel::Info, "reloading config...");
                    reload_inflight = Some((handlers.reload_handler)(bot));
                }
                needs_redraw = true;
            }
            _ = tick.tick() => {
                let mut ui_changed = false;
                let uptime_sec = app.started_at.elapsed().as_secs();
                if uptime_sec != last_uptime_sec {
                    last_uptime_sec = uptime_sec;
                    ui_changed = true;
                }

                if reload_inflight.is_none() && app.refresh_adapter_state(bot) {
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
                            if reload_inflight.is_some() {
                                queued_reload = true;
                                app.push_log(UiLevel::Warn, "reload already in progress, request queued");
                            } else {
                                app.push_log(UiLevel::Info, "reloading config...");
                                reload_inflight = Some((handlers.reload_handler)(bot));
                            }
                            ui_changed = true;
                        }
                        CommandOutcome::PersistWhitelist(entries) => {
                            match (handlers.whitelist_persist_handler)(entries) {
                                Ok(message) => {
                                    app.push_log(UiLevel::Info, message);
                                    ui_changed = true;
                                    if reload_inflight.is_some() {
                                        queued_reload = true;
                                        app.push_log(UiLevel::Warn, "reload already in progress, request queued");
                                    } else {
                                        app.push_log(UiLevel::Info, "reloading config...");
                                        reload_inflight = Some((handlers.reload_handler)(bot));
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
                            spawn_llm_command(
                                request,
                                handlers,
                                &async_result_tx,
                                &async_command_limiter,
                                app,
                            );
                            ui_changed = true;
                        }
                        CommandOutcome::Ask(prompt) => {
                            spawn_ask_command(
                                prompt,
                                handlers,
                                &async_result_tx,
                                &async_command_limiter,
                                app,
                            );
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
                apply_async_result(app, async_result);
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

fn spawn_llm_command(
    request: LlmCommandRequest,
    handlers: LoopHandlers,
    async_result_tx: &mpsc::Sender<AsyncCommandResult>,
    async_command_limiter: &Arc<Semaphore>,
    app: &mut AppState,
) {
    if let Ok(permit) = async_command_limiter.clone().try_acquire_owned() {
        let tx = async_result_tx.clone();
        let llm_command_handler = handlers.llm_command_handler;
        tokio::spawn(async move {
            let _permit = permit;
            let result = llm_command_handler(request).await;
            let _ = tx.send(AsyncCommandResult::Llm(result)).await;
        });
    } else {
        app.push_log(
            UiLevel::Warn,
            format!(
                "too many async commands in flight (limit={ASYNC_COMMAND_CONCURRENCY_LIMIT}), please retry"
            ),
        );
    }
}

fn spawn_ask_command(
    prompt: String,
    handlers: LoopHandlers,
    async_result_tx: &mpsc::Sender<AsyncCommandResult>,
    async_command_limiter: &Arc<Semaphore>,
    app: &mut AppState,
) {
    if let Ok(permit) = async_command_limiter.clone().try_acquire_owned() {
        let tx = async_result_tx.clone();
        let ask_handler = handlers.ask_handler;
        tokio::spawn(async move {
            let _permit = permit;
            let result = ask_handler(prompt).await;
            let _ = tx.send(AsyncCommandResult::Ask(result)).await;
        });
    } else {
        app.push_log(
            UiLevel::Warn,
            format!(
                "too many async commands in flight (limit={ASYNC_COMMAND_CONCURRENCY_LIMIT}), please retry"
            ),
        );
    }
}

fn apply_async_result(app: &mut AppState, async_result: AsyncCommandResult) {
    match async_result {
        AsyncCommandResult::Llm(result) => match result {
            Ok(message) => app.push_log(UiLevel::Info, message),
            Err(err) => app.push_log(UiLevel::Warn, format!("llm command failed: {err}")),
        },
        AsyncCommandResult::Ask(result) => match result {
            Ok(message) => push_llm_response_logs(app, message),
            Err(err) => app.push_log(UiLevel::Warn, format!("ask failed: {err}")),
        },
    }
}

fn push_llm_response_logs(app: &mut AppState, message: String) {
    let normalized = message.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.split('\n').peekable();
    if lines.peek().is_none() {
        app.push_log(UiLevel::Llm, " ");
        return;
    }

    let mut has_non_empty = false;
    let mut is_first = true;
    for line in lines {
        let mut content = line
            .chars()
            .filter(|ch| !ch.is_control() || *ch == '\t')
            .filter(|ch| {
                !matches!(
                    *ch,
                    '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}'
                )
            })
            .collect::<String>()
            .replace('\t', "    ");
        if content.is_empty() {
            content = " ".to_string();
        } else if content.trim().is_empty() {
            content = " ".to_string();
        } else {
            has_non_empty = true;
        }

        if is_first {
            app.push_log(UiLevel::Llm, content);
            is_first = false;
        } else {
            app.push_log(UiLevel::Llm, format!("| {content}"));
        }
    }
    if !has_non_empty {
        app.push_log(UiLevel::Llm, "(empty llm output)");
    }
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
        path.push(format!("rsliteyuki-runtime-{name}-{nanos}.json"));
        path
    }

    fn test_app() -> AppState {
        AppState::new(
            RuntimeTarget::Cli,
            "test".to_string(),
            Vec::new(),
            TuiConfig {
                resume_store_path: temp_resume_path("llm-multiline"),
                resume_max_sessions: 8,
                resume_max_size_mib: 4,
            },
        )
    }

    #[test]
    fn llm_multiline_output_is_split_into_multiple_logs() {
        let mut app = test_app();
        push_llm_response_logs(&mut app, "line-1\nline-2\nline-3".to_string());

        let tail: Vec<(UiLevel, String)> = app
            .logs
            .iter()
            .rev()
            .take(3)
            .map(|log| (log.level, log.message.clone()))
            .collect();
        assert_eq!(
            tail.into_iter().rev().collect::<Vec<_>>(),
            vec![
                (UiLevel::Llm, "line-1".to_string()),
                (UiLevel::Llm, "| line-2".to_string()),
                (UiLevel::Llm, "| line-3".to_string()),
            ]
        );
    }

    #[test]
    fn llm_multiline_output_preserves_blank_lines() {
        let mut app = test_app();
        push_llm_response_logs(&mut app, "first\n\nthird".to_string());

        let tail: Vec<String> = app
            .logs
            .iter()
            .rev()
            .take(3)
            .map(|log| log.message.clone())
            .collect();
        assert_eq!(
            tail.into_iter().rev().collect::<Vec<_>>(),
            vec![
                "first".to_string(),
                "|  ".to_string(),
                "| third".to_string(),
            ]
        );
    }

    #[test]
    fn llm_zero_width_output_shows_placeholder() {
        let mut app = test_app();
        push_llm_response_logs(&mut app, "\u{200B}\u{200D}\u{FEFF}".to_string());

        let tail: Vec<String> = app
            .logs
            .iter()
            .rev()
            .take(2)
            .map(|log| log.message.clone())
            .collect();
        assert_eq!(
            tail.into_iter().rev().collect::<Vec<_>>(),
            vec![" ".to_string(), "(empty llm output)".to_string(),]
        );
    }
}
