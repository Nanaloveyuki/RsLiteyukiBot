use super::*;

#[derive(Clone, Copy)]
struct LoopHandlers {
    reload_handler: ReloadHandler,
    whitelist_persist_handler: PersistWhitelistHandler,
    llm_command_handler: LlmCommandHandler,
    ask_handler: AskHandler,
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

    let _ = restore_terminal(&mut terminal);
    set_console_log_output_enabled(previous_console_log_output);
    app.flush_resume_if_needed(true);
    loop_result
}

async fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    bot: &mut LiteyukiBot,
    app: &mut AppState,
    handlers: LoopHandlers,
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
                            match (handlers.reload_handler)(bot).await {
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
                            match (handlers.whitelist_persist_handler)(entries) {
                                Ok(message) => {
                                    app.push_log(UiLevel::Info, message);
                                    app.push_log(UiLevel::Info, "reloading config...");
                                    ui_changed = true;
                                    match (handlers.reload_handler)(bot).await {
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
                            let llm_command_handler = handlers.llm_command_handler;
                            tokio::spawn(async move {
                                let result = llm_command_handler(request).await;
                                let _ = tx.send(AsyncCommandResult::Llm(result));
                            });
                            ui_changed = true;
                        }
                        CommandOutcome::Ask(prompt) => {
                            let tx = async_result_tx.clone();
                            let ask_handler = handlers.ask_handler;
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
