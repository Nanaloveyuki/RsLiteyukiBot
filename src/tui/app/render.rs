use super::*;
use liteyukibot_core::{PluginCatalogEntry, PluginLoadState};

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
        .constraints([Constraint::Percentage(26), Constraint::Percentage(74)])
        .split(root[1]);

    let sidebar = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(mid[0]);
    render_adapters_panel(frame, app, sidebar[0]);
    render_plugins_panel(frame, app, sidebar[1]);

    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Min(3),
            Constraint::Length(4),
        ])
        .split(mid[1]);
    let dashboard_top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(content[0]);
    render_external_panel(frame, app, dashboard_top[0]);
    render_plugin_detail_panel(frame, app, dashboard_top[1]);
    render_logs_panel(frame, app, content[1], "Console");
    render_command_panel(frame, app, content[2]);

    let footer = Paragraph::new(dashboard_footer_text(app)).wrap(Wrap { trim: true });
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

    let footer = Paragraph::new(
        "日志视图: 空输入时 Up/Down 或 PgUp/PgDn/Home/End 滚动日志；Ctrl+Up/Down 历史；Tab 补全；Ctrl+C 退出",
    )
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn panel_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    let mut block = rounded_block(title);
    if focused {
        block = block.border_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    }
    block
}

fn render_adapters_panel(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let adapter_text_width = (area.width as usize).saturating_sub(2).max(1);
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
    frame.render_widget(adapter_list, area);
}

fn render_plugins_panel(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let catalog = app.plugin_catalog_entries();
    let (total_plugins, enabled_plugins, loaded_plugins) = app.dashboard_plugin_summary_counts();
    let selected_index = app.normalized_dashboard_plugin_index(catalog.len());
    let detail_width = (area.width as usize).saturating_sub(5).max(1);
    let focused = app.is_dashboard_plugins_focus();
    let items: Vec<ListItem<'_>> = if catalog.is_empty() {
        vec![ListItem::new(Line::from("no plugins discovered"))]
    } else {
        catalog
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let plugin_id = entry.descriptor.metadata.id.clone();
                let enabled = app.is_plugin_enabled(plugin_id.as_str());
                let selected = selected_index == Some(index);
                let marker_style = if selected && focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let title_style = if selected && focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let enabled_style = if enabled {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                let state_label = plugin_load_state_label(&entry);
                let mut lines = vec![Line::from(vec![
                    Span::styled(if selected { "> " } else { "  " }, marker_style),
                    Span::styled(
                        format!("[{}] ", if enabled { "ON" } else { "OFF" }),
                        enabled_style,
                    ),
                    Span::styled(
                        format!(
                            "{} ({})",
                            plugin_id,
                            AppState::plugin_runtime_label(entry.descriptor.runtime.kind)
                        ),
                        title_style,
                    ),
                ])];
                let summary = format!(
                    "{} | {} | {}",
                    entry.descriptor.metadata.name,
                    AppState::plugin_type_label(entry.descriptor.metadata.plugin_type),
                    state_label
                );
                for detail_line in wrap_text_hard(&summary, detail_width) {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(detail_line, Style::default().fg(Color::DarkGray)),
                    ]));
                }
                ListItem::new(Text::from(lines))
            })
            .collect()
    };
    let title = if focused {
        format!("Plugins ({enabled_plugins}/{total_plugins} on, {loaded_plugins} loaded) [focus]")
    } else {
        format!("Plugins ({enabled_plugins}/{total_plugins} on, {loaded_plugins} loaded)")
    };
    let plugin_list = List::new(items).block(panel_block(title.as_str(), focused));
    frame.render_widget(plugin_list, area);
}

fn plugin_load_state_label(entry: &PluginCatalogEntry) -> &'static str {
    match entry.load_state {
        Some(PluginLoadState::Ready) => "ready",
        Some(PluginLoadState::Deferred) => "deferred",
        None if entry.loaded => "loaded",
        None => "not-loaded",
    }
}

fn render_plugin_detail_panel(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let focused = app.is_dashboard_plugins_focus();
    let Some(entry) = app.selected_dashboard_plugin_entry() else {
        let placeholder = Paragraph::new(
            "未发现插件。可继续使用 /plugins 查看目录，或等待下一次 reload 后刷新插件列表。",
        )
        .block(panel_block("Plugin Detail", focused))
        .wrap(Wrap { trim: true });
        frame.render_widget(placeholder, area);
        return;
    };

    let plugin_id = entry.descriptor.metadata.id.clone();
    let enabled = app.is_plugin_enabled(plugin_id.as_str());
    let state_label = plugin_load_state_label(&entry);
    let state_style = match state_label {
        "ready" => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        "deferred" => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        "loaded" => Style::default()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(Color::DarkGray),
    };
    let enabled_style = if enabled {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    let runtime = &entry.descriptor.runtime;
    let metadata = &entry.descriptor.metadata;
    let sdk = &entry.descriptor.sdk;
    let load_reason = entry
        .load_reason
        .clone()
        .filter(|reason| !reason.trim().is_empty())
        .unwrap_or_else(|| {
            if !enabled {
                "disabled by current plugin policy".to_string()
            } else if entry.loaded {
                "loaded without extra runtime note".to_string()
            } else {
                "waiting for next plugin reload/start".to_string()
            }
        });
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("[{}] ", if enabled { "ON" } else { "OFF" }),
            enabled_style,
        ),
        Span::styled(format!("[{}] ", state_label.to_uppercase()), state_style),
        Span::styled(
            format!(
                "{} ({})",
                plugin_id,
                AppState::plugin_runtime_label(runtime.kind)
            ),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])];

    let title = if metadata.name.trim().is_empty() {
        "name: <unnamed>".to_string()
    } else {
        format!("name: {}", metadata.name)
    };
    lines.push(Line::from(title));

    if !metadata.description.trim().is_empty() {
        lines.push(Line::from(format!("desc: {}", metadata.description)));
    }

    let origin = if entry.descriptor.manifest_path.is_some() {
        "manifest"
    } else {
        "registered"
    };
    let min_host_version = if sdk.min_host_version.trim().is_empty() {
        "any".to_string()
    } else {
        sdk.min_host_version.clone()
    };
    lines.push(Line::from(format!(
        "type: {} | origin: {} | sdk: api {} / host {}",
        AppState::plugin_type_label(metadata.plugin_type),
        origin,
        sdk.api_version,
        min_host_version
    )));

    let mut runtime_parts = Vec::new();
    if !runtime.entrypoint.trim().is_empty() {
        runtime_parts.push(format!("entry {}", runtime.entrypoint));
    }
    if !runtime.module.trim().is_empty() {
        runtime_parts.push(format!("module {}", runtime.module));
    }
    if !runtime.abi.trim().is_empty() {
        runtime_parts.push(format!("abi {}", runtime.abi));
    }
    if !runtime.min_version.trim().is_empty() {
        runtime_parts.push(format!("min {}", runtime.min_version));
    }
    if !runtime_parts.is_empty() {
        lines.push(Line::from(format!(
            "runtime: {}",
            runtime_parts.join(" | ")
        )));
    }

    if let Some(manifest_path) = entry.descriptor.manifest_path.as_ref() {
        lines.push(Line::from(format!("manifest: {}", manifest_path.display())));
    }

    let permissions = if entry.descriptor.permissions.is_empty() {
        "none declared".to_string()
    } else {
        entry.descriptor.permissions.join(", ")
    };
    let commands = if entry.descriptor.commands.is_empty() {
        "no manifest commands".to_string()
    } else {
        entry
            .descriptor
            .commands
            .iter()
            .map(|command| command.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    lines.push(Line::from(format!("permissions: {permissions}")));
    lines.push(Line::from(format!("commands: {commands}")));
    lines.push(Line::from(format!("reason: {load_reason}")));
    lines.push(Line::from(format!(
        "next: Enter to {} | Tab to command | /plugins list for log view",
        if enabled { "disable" } else { "enable" }
    )));

    let widget = Paragraph::new(Text::from(lines))
        .block(panel_block("Plugin Detail", focused))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn render_external_panel(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let success_rate = if app.external_api_requests == 0 {
        0.0
    } else {
        (app.external_api_success as f64) / (app.external_api_requests as f64) * 100.0
    };
    let (total_plugins, enabled_plugins, loaded_plugins) = app.dashboard_plugin_summary_counts();

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
    let line3 = Line::from(vec![
        Span::styled("plugins=", Style::default().fg(Color::DarkGray)),
        Span::styled(total_plugins.to_string(), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("enabled=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            enabled_plugins.to_string(),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled("loaded=", Style::default().fg(Color::DarkGray)),
        Span::styled(
            loaded_plugins.to_string(),
            Style::default().fg(Color::LightBlue),
        ),
    ]);

    let widget = Paragraph::new(Text::from(vec![line1, line2, line3]))
        .block(rounded_block("External EventHandle"))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn dashboard_footer_text(app: &AppState) -> String {
    let focus_hint = if app.is_dashboard_plugin_panel_active() {
        if let Some(entry) = app.selected_dashboard_plugin_entry() {
            let plugin_id = entry.descriptor.metadata.id;
            let action = if app.is_plugin_enabled(plugin_id.as_str()) {
                "禁用"
            } else {
                "启用"
            };
            format!("焦点=插件({plugin_id}) Enter {action}")
        } else {
            "焦点=插件".to_string()
        }
    } else {
        "焦点=命令 Enter 执行".to_string()
    };
    format!(
        "{focus_hint}  |  Tab 切换命令/插件  |  Up/Down 历史或插件选择  |  PgUp/PgDn/Home/End 日志滚动  |  Ctrl+C 退出  |  settings: {}",
        app.settings_desc
    )
}

fn render_logs_panel(frame: &mut ratatui::Frame<'_>, app: &mut AppState, area: Rect, title: &str) {
    let log_rows = (area.height as usize).saturating_sub(2).max(1);
    let log_text_width = (area.width as usize).saturating_sub(2).max(1);
    app.set_log_view_rows(log_rows);
    app.set_log_text_width(log_text_width);
    let max_scroll = app.max_log_scroll();
    let scroll = app.log_scroll.min(max_scroll);
    let rendered_lines: Vec<Line<'_>> = app
        .logs
        .iter()
        .flat_map(|log| {
            let (tag_style, message_style) = match log.level {
                UiLevel::Info => (
                    Style::default().fg(Color::Blue),
                    Style::default().fg(Color::White),
                ),
                UiLevel::Warn => (
                    Style::default().fg(Color::Yellow),
                    Style::default().fg(Color::Yellow),
                ),
                UiLevel::Error => (
                    Style::default().fg(Color::Red),
                    Style::default().fg(Color::Red),
                ),
                UiLevel::Event => (
                    Style::default().fg(Color::Green),
                    Style::default().fg(Color::Green),
                ),
                UiLevel::Llm => (
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                    Style::default()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::BOLD),
                ),
            };
            let tag = log_level_tag(log.level);
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
                Span::styled(format!("[{}] ", tag), tag_style),
                Span::styled(first_line, message_style),
            ])];
            let indent = " ".repeat(prefix_width);
            for segment in wrapped_message.iter().skip(1) {
                lines.push(Line::from(vec![
                    Span::raw(indent.clone()),
                    Span::styled(segment.clone(), message_style),
                ]));
            }
            lines
        })
        .collect();
    let total_lines = rendered_lines.len();
    let end = total_lines.saturating_sub(scroll);
    let start = end.saturating_sub(log_rows);
    let visible_lines = if start < end {
        rendered_lines[start..end].to_vec()
    } else {
        Vec::new()
    };
    let panel_title = if scroll > 0 {
        format!("{title} (scroll {}/{max_scroll})", scroll)
    } else {
        title.to_string()
    };
    let logs_widget =
        Paragraph::new(Text::from(visible_lines)).block(rounded_block(panel_title.as_str()));
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
    let input_widget = Paragraph::new(Line::from(spans))
        .block(panel_block("Command", !app.is_dashboard_plugins_focus()));
    frame.render_widget(input_widget, input_area);
    if !app.is_dashboard_plugins_focus() {
        let (cursor_x, cursor_y) = command_cursor_position(input_area, app.console_input.as_str());
        frame.set_cursor_position((cursor_x, cursor_y));
    }
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
