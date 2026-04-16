use super::*;

pub(super) fn draw_ui(frame: &mut ratatui::Frame<'_>, app: &mut AppState) {
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

pub(super) fn command_cursor_position(area: Rect, input: &str) -> (u16, u16) {
    let inner_x = area.x.saturating_add(1);
    let inner_y = area.y.saturating_add(1);
    let available_width = area.width.saturating_sub(2) as usize;
    let prompt_width = UnicodeWidthStr::width("> ");
    let input_width = UnicodeWidthStr::width(input);
    let max_offset = available_width.saturating_sub(1);
    let offset = (prompt_width + input_width).min(max_offset) as u16;
    (inner_x.saturating_add(offset), inner_y)
}
