use super::*;
use crate::i18n::tr;

pub(super) fn poll_key_events(
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
            app.push_log(UiLevel::Warn, tr("tui.signal.ctrl_c.key_event"));
            *should_quit = true;
            had_ui_change = true;
            continue;
        }

        match key.code {
            KeyCode::Enter => {
                if app.is_dashboard_plugin_panel_active() {
                    if let Some(command) = app.dashboard_toggle_selected_plugin_command() {
                        submitted.push(command);
                        had_ui_change = true;
                    }
                    continue;
                }
                had_ui_change = true;
                let input = app.console_input.trim().to_string();
                if !input.is_empty() {
                    let display_input = app.redact_console_command_for_display(&input);
                    app.push_log(UiLevel::Info, format!("> {}", display_input));
                    if app.should_record_command_history(&input) {
                        app.record_command(&input);
                    }
                    submitted.push(input);
                }
                app.console_input.clear();
                app.reset_history_navigation();
            }
            KeyCode::Backspace => {
                had_ui_change = true;
                app.focus_dashboard_command();
                app.detach_from_history_cursor();
                app.console_input.pop();
            }
            KeyCode::Up => {
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && app.is_dashboard_plugin_panel_active()
                {
                    had_ui_change |= app.move_dashboard_plugin_selection(-1);
                } else {
                    had_ui_change = true;
                    app.focus_dashboard_command();
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
            }
            KeyCode::Down => {
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && app.is_dashboard_plugin_panel_active()
                {
                    had_ui_change |= app.move_dashboard_plugin_selection(1);
                } else {
                    had_ui_change = true;
                    app.focus_dashboard_command();
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
            }
            KeyCode::Tab => {
                had_ui_change = true;
                if app.is_dashboard_view() && app.console_input.is_empty() {
                    app.cycle_dashboard_focus();
                } else {
                    app.focus_dashboard_command();
                    app.autocomplete_console_input();
                }
            }
            KeyCode::PageUp => {
                had_ui_change = true;
                app.focus_dashboard_command();
                app.clear_completion_state();
                app.scroll_logs_page_up();
            }
            KeyCode::PageDown => {
                had_ui_change = true;
                app.focus_dashboard_command();
                app.clear_completion_state();
                app.scroll_logs_page_down();
            }
            KeyCode::Home => {
                had_ui_change = true;
                app.focus_dashboard_command();
                app.clear_completion_state();
                app.scroll_logs_top();
            }
            KeyCode::End => {
                had_ui_change = true;
                app.focus_dashboard_command();
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
                app.focus_dashboard_command();
                app.detach_from_history_cursor();
                app.console_input.push(ch);
            }
            KeyCode::Esc => {
                had_ui_change = true;
                if app.is_dashboard_plugin_panel_active() {
                    app.focus_dashboard_command();
                } else {
                    app.console_input.clear();
                    app.reset_history_navigation();
                    app.focus_dashboard_command();
                }
            }
            _ => {}
        }
    }
    Ok(PollKeyEventsOutput {
        submitted_commands: submitted,
        had_ui_change,
    })
}
