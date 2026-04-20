use super::*;
use crate::i18n::{tr, trf};
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
    let uptime_text = uptime.to_string();
    let events_text = app.total_events.to_string();
    let adapter_events_text = app.adapter_events.to_string();
    let external_commands_text = app.external_command_hits.to_string();
    let api_inflight_text = app.external_api_inflight.to_string();
    let target_text = format!("{:?}", app.target);
    let runtime_panel_title = tr("tui.panel.runtime");
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {} ", tr("tui.brand")),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(trf(
            "tui.header.metrics",
            &[
                ("target", target_text.as_str()),
                ("uptime", uptime_text.as_str()),
                ("events", events_text.as_str()),
                ("adapter_events", adapter_events_text.as_str()),
                ("external_commands", external_commands_text.as_str()),
                ("api_inflight", api_inflight_text.as_str()),
                ("resume", app.active_resume_uid()),
            ],
        )),
    ]))
    .block(rounded_block(runtime_panel_title.as_str()));
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
    render_logs_panel(frame, app, content[1], tr("tui.panel.console").as_str());
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
    render_logs_panel(frame, app, content[0], tr("tui.panel.log").as_str());
    render_command_panel(frame, app, content[1]);

    let footer = Paragraph::new(tr("tui.footer.log_view")).wrap(Wrap { trim: true });
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
        vec![ListItem::new(Line::from(tr("tui.adapters.empty")))]
    } else {
        app.adapters
            .iter()
            .map(|adapter| {
                let running = app
                    .adapter_running
                    .get(&adapter.id)
                    .copied()
                    .unwrap_or(false);
                let status = if running {
                    tr("tui.adapter.status.run")
                } else {
                    tr("tui.adapter.status.idle")
                };
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
    let adapters_panel_title = tr("tui.panel.adapters");
    let adapter_list = List::new(adapter_items).block(rounded_block(adapters_panel_title.as_str()));
    frame.render_widget(adapter_list, area);
}

fn render_plugins_panel(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let catalog = app.plugin_catalog_entries();
    let (total_plugins, enabled_plugins, loaded_plugins) = app.dashboard_plugin_summary_counts();
    let selected_index = app.normalized_dashboard_plugin_index(catalog.len());
    let detail_width = (area.width as usize).saturating_sub(5).max(1);
    let focused = app.is_dashboard_plugins_focus();
    let items: Vec<ListItem<'_>> = if catalog.is_empty() {
        vec![ListItem::new(Line::from(tr("tui.plugins.none_discovered")))]
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
                let enabled_badge = if enabled {
                    tr("tui.toggle.on")
                } else {
                    tr("tui.toggle.off")
                };
                let state_label = plugin_load_state_label(&entry);
                let mut lines = vec![Line::from(vec![
                    Span::styled(if selected { "> " } else { "  " }, marker_style),
                    Span::styled(format!("[{}] ", enabled_badge), enabled_style),
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
                    tr(entry.descriptor.metadata.name.as_str()),
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
    let enabled_text = enabled_plugins.to_string();
    let total_text = total_plugins.to_string();
    let loaded_text = loaded_plugins.to_string();
    let title = if focused {
        trf(
            "tui.plugins.title.focused",
            &[
                ("enabled", enabled_text.as_str()),
                ("total", total_text.as_str()),
                ("loaded", loaded_text.as_str()),
            ],
        )
    } else {
        trf(
            "tui.plugins.title",
            &[
                ("enabled", enabled_text.as_str()),
                ("total", total_text.as_str()),
                ("loaded", loaded_text.as_str()),
            ],
        )
    };
    let plugin_list = List::new(items).block(panel_block(title.as_str(), focused));
    frame.render_widget(plugin_list, area);
}

fn plugin_load_state_label(entry: &PluginCatalogEntry) -> String {
    match entry.load_state {
        Some(PluginLoadState::Ready) => tr("plugin.state.ready"),
        Some(PluginLoadState::Deferred) => tr("plugin.state.deferred"),
        None if entry.loaded => tr("plugin.state.loaded"),
        None => tr("plugin.state.not_loaded"),
    }
}

fn render_plugin_detail_panel(frame: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let focused = app.is_dashboard_plugins_focus();
    let plugin_detail_title = tr("tui.panel.plugin_detail");
    let Some(entry) = app.selected_dashboard_plugin_entry() else {
        let placeholder = Paragraph::new(tr("tui.plugins.empty"))
            .block(panel_block(plugin_detail_title.as_str(), focused))
            .wrap(Wrap { trim: true });
        frame.render_widget(placeholder, area);
        return;
    };

    let plugin_id = entry.descriptor.metadata.id.clone();
    let enabled = app.is_plugin_enabled(plugin_id.as_str());
    let state_label = plugin_load_state_label(&entry);
    let state_style = match state_label.as_str() {
        value if value == tr("plugin.state.ready") => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        value if value == tr("plugin.state.deferred") => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        value if value == tr("plugin.state.loaded") => Style::default()
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
                tr("plugin.detail.reason.disabled_policy")
            } else if entry.loaded {
                tr("plugin.detail.reason.loaded_without_note")
            } else {
                tr("plugin.detail.reason.waiting_reload")
            }
        });
    let mut lines = vec![Line::from(vec![
        {
            let enabled_badge = if enabled {
                tr("tui.toggle.on")
            } else {
                tr("tui.toggle.off")
            };
            Span::styled(format!("[{}] ", enabled_badge), enabled_style)
        },
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

    let translated_name = tr(metadata.name.as_str());
    let title = if metadata.name.trim().is_empty() {
        tr("plugin.detail.name.unnamed")
    } else {
        trf("plugin.detail.name", &[("name", translated_name.as_str())])
    };
    lines.push(Line::from(title));

    if !metadata.description.trim().is_empty() {
        let description = tr(metadata.description.as_str());
        lines.push(Line::from(trf(
            "plugin.detail.description",
            &[("description", description.as_str())],
        )));
    }

    let origin = if entry.descriptor.manifest_path.is_some() {
        tr("plugin.detail.origin.manifest")
    } else {
        tr("plugin.detail.origin.registered")
    };
    let min_host_version = if sdk.min_host_version.trim().is_empty() {
        tr("plugin.detail.host.any")
    } else {
        sdk.min_host_version.clone()
    };
    let plugin_type = AppState::plugin_type_label(metadata.plugin_type);
    lines.push(Line::from(trf(
        "plugin.detail.type_line",
        &[
            ("type", plugin_type.as_str()),
            ("origin", origin.as_str()),
            ("api_version", sdk.api_version.as_str()),
            ("host_version", min_host_version.as_str()),
        ],
    )));

    let mut runtime_parts = Vec::new();
    if !runtime.entrypoint.trim().is_empty() {
        runtime_parts.push(trf(
            "plugin.detail.runtime.entry",
            &[("value", runtime.entrypoint.as_str())],
        ));
    }
    if !runtime.module.trim().is_empty() {
        runtime_parts.push(trf(
            "plugin.detail.runtime.module",
            &[("value", runtime.module.as_str())],
        ));
    }
    if !runtime.abi.trim().is_empty() {
        runtime_parts.push(trf(
            "plugin.detail.runtime.abi",
            &[("value", runtime.abi.as_str())],
        ));
    }
    if !runtime.min_version.trim().is_empty() {
        runtime_parts.push(trf(
            "plugin.detail.runtime.min",
            &[("value", runtime.min_version.as_str())],
        ));
    }
    if !runtime_parts.is_empty() {
        lines.push(Line::from(trf(
            "plugin.detail.runtime_line",
            &[("runtime", runtime_parts.join(" | ").as_str())],
        )));
    }

    if let Some(manifest_path) = entry.descriptor.manifest_path.as_ref() {
        lines.push(Line::from(trf(
            "plugin.detail.manifest",
            &[("path", manifest_path.display().to_string().as_str())],
        )));
    }

    let permissions = if entry.descriptor.permissions.is_empty() {
        tr("plugin.detail.permissions.none")
    } else {
        entry.descriptor.permissions.join(", ")
    };
    let commands = if entry.descriptor.commands.is_empty() {
        tr("plugin.detail.commands.none")
    } else {
        entry
            .descriptor
            .commands
            .iter()
            .map(|command| command.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    lines.push(Line::from(trf(
        "plugin.detail.permissions",
        &[("permissions", permissions.as_str())],
    )));
    lines.push(Line::from(trf(
        "plugin.detail.commands",
        &[("commands", commands.as_str())],
    )));
    lines.push(Line::from(trf(
        "plugin.detail.reason",
        &[("reason", load_reason.as_str())],
    )));
    let next_action = if enabled {
        tr("tui.action.disable")
    } else {
        tr("tui.action.enable")
    };
    lines.push(Line::from(trf(
        "plugin.detail.next",
        &[("action", next_action.as_str())],
    )));

    let widget = Paragraph::new(Text::from(lines))
        .block(panel_block(plugin_detail_title.as_str(), focused))
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
        Span::styled(
            tr("tui.external.commands"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            app.external_command_hits.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            tr("tui.external.inflight"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            app.external_api_inflight.to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let line2 = Line::from(vec![
        Span::styled(
            tr("tui.external.api_requests"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            app.external_api_requests.to_string(),
            Style::default().fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled(
            tr("tui.external.api_success"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            app.external_api_success.to_string(),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled(
            tr("tui.external.api_failed"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            app.external_api_failed.to_string(),
            Style::default().fg(Color::Red),
        ),
        Span::raw("  "),
        Span::styled(
            tr("tui.external.api_timeouts"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            app.external_api_timeouts.to_string(),
            Style::default().fg(Color::Magenta),
        ),
        Span::raw("  "),
        Span::styled(
            tr("tui.external.api_rate"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{success_rate:.1}%"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let line3 = Line::from(vec![
        Span::styled(
            tr("tui.external.plugins"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(total_plugins.to_string(), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(
            tr("tui.external.enabled"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            enabled_plugins.to_string(),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled(
            tr("tui.external.loaded"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            loaded_plugins.to_string(),
            Style::default().fg(Color::LightBlue),
        ),
    ]);

    let external_panel_title = tr("tui.panel.external");
    let widget = Paragraph::new(Text::from(vec![line1, line2, line3]))
        .block(rounded_block(external_panel_title.as_str()))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn dashboard_footer_text(app: &AppState) -> String {
    let focus_hint = if app.is_dashboard_plugin_panel_active() {
        if let Some(entry) = app.selected_dashboard_plugin_entry() {
            let plugin_id = entry.descriptor.metadata.id;
            let action = if app.is_plugin_enabled(plugin_id.as_str()) {
                tr("tui.action.disable")
            } else {
                tr("tui.action.enable")
            };
            tr("tui.footer.focus.plugin")
                .replace("{plugin}", plugin_id.as_str())
                .replace("{action}", action.as_str())
        } else {
            tr("tui.footer.focus.plugin.empty").to_string()
        }
    } else {
        tr("tui.footer.focus.command").to_string()
    };
    tr("tui.footer.dashboard")
        .replace("{focus}", focus_hint.as_str())
        .replace("{settings}", app.settings_desc.as_str())
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
        trf(
            "tui.panel.scroll",
            &[
                ("title", title),
                ("scroll", scroll.to_string().as_str()),
                ("max_scroll", max_scroll.to_string().as_str()),
            ],
        )
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
    let command_panel_title = tr("tui.panel.command");
    let input_widget = Paragraph::new(Line::from(spans)).block(panel_block(
        command_panel_title.as_str(),
        !app.is_dashboard_plugins_focus(),
    ));
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
