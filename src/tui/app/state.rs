use super::*;

impl AppState {
    pub(super) fn new(
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

    pub(super) fn drop_oldest_non_active_resume(&mut self) -> bool {
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

    pub(super) fn enforce_resume_limits(&mut self) {
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

    pub(super) fn active_resume_uid(&self) -> &str {
        &self.active_resume_uid
    }

    pub(super) fn apply_reload_result(&mut self, result: ReloadResult) {
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

    pub(super) fn push_log(&mut self, level: UiLevel, message: impl Into<String>) {
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

    pub(super) fn apply_event(&mut self, event: UiEvent) {
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

    pub(super) fn refresh_adapter_state(&mut self, bot: &LiteyukiBot) -> bool {
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

    pub(super) fn sync_active_resume_snapshot(&mut self) {
        self.resume_store.update_session(
            &self.active_resume_uid,
            &self.logs,
            &self.command_history,
        );
        self.enforce_resume_limits();
    }

    pub(super) fn flush_resume_if_needed(&mut self, force: bool) {
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

    pub(super) fn reset_history_navigation(&mut self) {
        self.history_cursor = None;
        self.history_draft.clear();
        self.clear_completion_state();
    }

    pub(super) fn detach_from_history_cursor(&mut self) {
        if self.history_cursor.is_some() {
            self.history_cursor = None;
            self.history_draft.clear();
        }
        self.clear_completion_state();
    }

    pub(super) fn record_command(&mut self, command: &str) {
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

    pub(super) fn recall_previous_command(&mut self) {
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

    pub(super) fn recall_next_command(&mut self) {
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

    pub(super) fn set_log_view_rows(&mut self, rows: usize) {
        self.log_view_rows = rows.max(1);
        self.clamp_log_scroll();
    }

    pub(super) fn max_log_scroll(&self) -> usize {
        self.logs.len().saturating_sub(self.log_view_rows)
    }

    pub(super) fn clamp_log_scroll(&mut self) {
        self.log_scroll = self.log_scroll.min(self.max_log_scroll());
    }

    pub(super) fn scroll_logs_up(&mut self, lines: usize) {
        self.log_scroll = self
            .log_scroll
            .saturating_add(lines)
            .min(self.max_log_scroll());
    }

    pub(super) fn scroll_logs_down(&mut self, lines: usize) {
        self.log_scroll = self.log_scroll.saturating_sub(lines);
    }

    pub(super) fn scroll_logs_top(&mut self) {
        self.log_scroll = self.max_log_scroll();
    }

    pub(super) fn scroll_logs_bottom(&mut self) {
        self.log_scroll = 0;
    }

    pub(super) fn scroll_logs_page_up(&mut self) {
        let delta = self.log_view_rows.saturating_div(2).max(1);
        self.scroll_logs_up(delta);
    }

    pub(super) fn scroll_logs_page_down(&mut self) {
        let delta = self.log_view_rows.saturating_div(2).max(1);
        self.scroll_logs_down(delta);
    }

    pub(super) fn is_log_console_view(&self) -> bool {
        self.view_mode == UiViewMode::LogConsole
    }

    pub(super) fn set_view_mode(&mut self, view_mode: UiViewMode) {
        self.view_mode = view_mode;
    }
}
