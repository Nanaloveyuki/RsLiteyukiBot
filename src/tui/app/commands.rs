use super::*;

impl AppState {
    pub(super) fn bind_help_whitelist(&mut self, whitelist: Arc<RwLock<HashSet<String>>>) {
        self.help_whitelist = Some(whitelist);
    }

    pub(super) fn bind_llm_command_prefix(&mut self, command_prefix: Arc<RwLock<String>>) {
        self.llm_command_prefix = Some(command_prefix);
    }

    pub(super) fn show_whitelist_usage(&mut self) {
        self.push_log(
            UiLevel::Warn,
            "usage: /whitelist list | /whitelist add <id|scope:id|scope id> | /whitelist remove <id|scope:id|scope id>",
        );
    }

    pub(super) fn parse_whitelist_scope(scope: &str) -> Option<&'static str> {
        match scope.trim().to_ascii_lowercase().as_str() {
            "private" => Some("private"),
            "group" | "gourp" => Some("group"),
            "session" => Some("session"),
            "user" => Some("user"),
            _ => None,
        }
    }

    pub(super) fn parse_whitelist_scoped_entry(scope: &str, id: &str) -> Result<String, String> {
        let Some(scope) = Self::parse_whitelist_scope(scope) else {
            return Err("scope should be private|group|session|user".to_string());
        };
        let id = id.trim();
        if id.is_empty() {
            return Err("id should not be empty".to_string());
        }
        Ok(format!("{scope}:{id}"))
    }

    pub(super) fn parse_whitelist_entry(raw: &str) -> Result<String, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("entry should not be empty".to_string());
        }
        if let Some((scope, id)) = raw.split_once(':') {
            return Self::parse_whitelist_scoped_entry(scope, id);
        }
        Ok(raw.to_string())
    }

    fn with_whitelist_read<F>(&mut self, mut f: F)
    where
        F: FnMut(&HashSet<String>, &mut Self),
    {
        let Some(shared) = self.help_whitelist.clone() else {
            self.push_log(
                UiLevel::Warn,
                "whitelist bridge unavailable in current runtime",
            );
            return;
        };
        let Ok(lock) = shared.read() else {
            self.push_log(UiLevel::Error, "failed to lock whitelist (poisoned)");
            return;
        };
        f(&lock, self);
    }

    pub(super) fn handle_whitelist_command(&mut self, args: &[&str]) -> CommandOutcome {
        let Some(subcommand) = args.first().copied() else {
            self.show_whitelist_usage();
            return CommandOutcome::None;
        };
        match subcommand {
            "list" => {
                if args.len() != 1 {
                    self.show_whitelist_usage();
                    return CommandOutcome::None;
                }
                self.with_whitelist_read(|set, app| {
                    if set.is_empty() {
                        app.push_log(
                            UiLevel::Info,
                            "external /help whitelist empty (allow all sessions)",
                        );
                        return;
                    }
                    let mut entries: Vec<String> = set.iter().cloned().collect();
                    entries.sort();
                    app.push_log(
                        UiLevel::Info,
                        format!("external /help whitelist entries ({}):", entries.len()),
                    );
                    for entry in entries {
                        app.push_log(UiLevel::Info, format!("  - {entry}"));
                    }
                });
                CommandOutcome::None
            }
            "add" | "remove" => {
                let entry = match args {
                    [_, value] => Self::parse_whitelist_entry(value),
                    [_, scope, id] => Self::parse_whitelist_scoped_entry(scope, id),
                    _ => {
                        self.show_whitelist_usage();
                        return CommandOutcome::None;
                    }
                };
                let Ok(entry) = entry else {
                    self.show_whitelist_usage();
                    return CommandOutcome::None;
                };
                let Some(shared) = self.help_whitelist.clone() else {
                    self.push_log(
                        UiLevel::Warn,
                        "whitelist bridge unavailable in current runtime",
                    );
                    return CommandOutcome::None;
                };
                let Ok(mut lock) = shared.write() else {
                    self.push_log(UiLevel::Error, "failed to lock whitelist (poisoned)");
                    return CommandOutcome::None;
                };

                if subcommand == "add" {
                    if lock.insert(entry.clone()) {
                        let mut entries: Vec<String> = lock.iter().cloned().collect();
                        entries.sort();
                        self.push_log(UiLevel::Info, format!("whitelist added: {entry}"));
                        self.push_log(UiLevel::Info, "persisting whitelist and auto reloading...");
                        return CommandOutcome::PersistWhitelist(entries);
                    }
                    self.push_log(UiLevel::Info, format!("whitelist already exists: {entry}"));
                    CommandOutcome::None
                } else {
                    if lock.remove(entry.as_str()) {
                        let mut entries: Vec<String> = lock.iter().cloned().collect();
                        entries.sort();
                        self.push_log(UiLevel::Info, format!("whitelist removed: {entry}"));
                        self.push_log(UiLevel::Info, "persisting whitelist and auto reloading...");
                        return CommandOutcome::PersistWhitelist(entries);
                    }
                    self.push_log(UiLevel::Info, format!("whitelist not found: {entry}"));
                    CommandOutcome::None
                }
            }
            _ => {
                self.show_whitelist_usage();
                CommandOutcome::None
            }
        }
    }

    pub(super) fn show_llm_usage(&mut self) {
        self.push_log(
            UiLevel::Warn,
            "usage: /llm model <name> | /llm apikey <k1> [k2 ...] | /llm provider [name] | /llm provider add|remove|use <base-url> | /llm provider list | /llm on|off|enable|disable [provider] | /llm prompt list|use <name>|set <name> <soul>|remove <name>|preview [user_prompt]",
        );
    }

    pub(super) fn parse_llm_provider(raw: &str) -> Option<String> {
        let provider = raw.trim().to_ascii_lowercase();
        if provider.is_empty() {
            None
        } else {
            Some(provider)
        }
    }

    pub(super) fn parse_llm_provider_url(raw: &str) -> Option<String> {
        let url = raw.trim().trim_end_matches('/').to_string();
        if url.is_empty() {
            return None;
        }
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return None;
        }
        Some(url)
    }

    pub(super) fn parse_llm_api_keys(args: &[&str]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        for value in args {
            let key = value.trim().to_string();
            if key.is_empty() {
                continue;
            }
            if seen.insert(key.clone()) {
                keys.push(key);
            }
        }
        keys
    }

    fn is_sensitive_console_command(command: &str) -> bool {
        let mut parts = command.split_whitespace();
        matches!((parts.next(), parts.next()), (Some("/llm"), Some("apikey")))
    }

    pub(super) fn should_record_command_history(&self, command: &str) -> bool {
        !Self::is_sensitive_console_command(command)
    }

    pub(super) fn redact_console_command_for_display(&self, command: &str) -> String {
        if !Self::is_sensitive_console_command(command) {
            return command.to_string();
        }

        let key_count = command
            .split_whitespace()
            .skip(2)
            .filter(|token| !token.is_empty())
            .count();
        if key_count == 0 {
            "/llm apikey <redacted>".to_string()
        } else {
            format!("/llm apikey <redacted:{key_count}>")
        }
    }

    pub(super) fn handle_llm_command(&mut self, args: &[&str]) -> CommandOutcome {
        let Some(subcommand) = args.first().copied() else {
            self.show_llm_usage();
            return CommandOutcome::None;
        };

        match subcommand {
            "model" => {
                let model = match args {
                    [_, model] => model.trim(),
                    _ => {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    }
                };
                if model.is_empty() {
                    self.show_llm_usage();
                    return CommandOutcome::None;
                }
                self.push_log(UiLevel::Info, format!("updating llm.model -> {model}"));
                CommandOutcome::Llm(LlmCommandRequest::SetModel(model.to_string()))
            }
            "apikey" => {
                let keys = Self::parse_llm_api_keys(&args[1..]);
                if keys.is_empty() {
                    self.show_llm_usage();
                    return CommandOutcome::None;
                }
                self.push_log(
                    UiLevel::Info,
                    format!("adding {} api key(s) to llm.api_keys", keys.len()),
                );
                CommandOutcome::Llm(LlmCommandRequest::AddApiKeys(keys))
            }
            "provider" => match args {
                [_, "list"] => {
                    self.push_log(UiLevel::Info, "listing llm provider base-url(s) ...");
                    CommandOutcome::Llm(LlmCommandRequest::ListProviderUrls)
                }
                [_, "add", provider_url] => {
                    let Some(provider_url) = Self::parse_llm_provider_url(provider_url) else {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    };
                    self.push_log(
                        UiLevel::Info,
                        format!("adding llm provider base-url -> {provider_url}"),
                    );
                    CommandOutcome::Llm(LlmCommandRequest::AddProviderUrl(provider_url))
                }
                [_, "remove", provider_url] => {
                    let Some(provider_url) = Self::parse_llm_provider_url(provider_url) else {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    };
                    self.push_log(
                        UiLevel::Info,
                        format!("removing llm provider base-url -> {provider_url}"),
                    );
                    CommandOutcome::Llm(LlmCommandRequest::RemoveProviderUrl(provider_url))
                }
                [_, "use", provider_url] => {
                    let Some(provider_url) = Self::parse_llm_provider_url(provider_url) else {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    };
                    self.push_log(
                        UiLevel::Info,
                        format!("switching llm provider base-url -> {provider_url}"),
                    );
                    CommandOutcome::Llm(LlmCommandRequest::UseProviderUrl(provider_url))
                }
                [_, "add" | "remove" | "use"] => {
                    self.show_llm_usage();
                    CommandOutcome::None
                }
                [_] | [_, _] => {
                    let provider = match args {
                        [_] => None,
                        [_, provider] => Self::parse_llm_provider(provider),
                        _ => None,
                    };
                    if let Some(provider) = provider.as_deref() {
                        self.push_log(
                            UiLevel::Info,
                            format!("probing llm provider (override={provider}) ..."),
                        );
                    } else {
                        self.push_log(UiLevel::Info, "probing current llm provider ...");
                    }
                    CommandOutcome::Llm(LlmCommandRequest::ProbeProvider(provider))
                }
                _ => {
                    self.show_llm_usage();
                    CommandOutcome::None
                }
            },
            "enable" | "on" | "disable" | "off" => {
                let enabled = matches!(subcommand, "enable" | "on");
                let provider = match args {
                    [_] => None,
                    [_, provider] => Self::parse_llm_provider(provider),
                    _ => {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    }
                };
                self.push_log(
                    UiLevel::Info,
                    format!(
                        "setting llm {}{}",
                        if enabled { "enabled" } else { "disabled" },
                        provider
                            .as_deref()
                            .map(|provider| format!(" (provider={provider})"))
                            .unwrap_or_default()
                    ),
                );
                CommandOutcome::Llm(LlmCommandRequest::SetEnabled { enabled, provider })
            }
            "prompt" => match args {
                [_, "list"] => {
                    self.push_log(UiLevel::Info, "listing llm prompt profiles...");
                    CommandOutcome::Llm(LlmCommandRequest::PromptList)
                }
                [_, "use", name] => {
                    let name = name.trim();
                    if name.is_empty() {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    }
                    self.push_log(
                        UiLevel::Info,
                        format!("switching llm prompt profile -> {name}"),
                    );
                    CommandOutcome::Llm(LlmCommandRequest::PromptUse(name.to_string()))
                }
                [_, "set", name, soul @ ..] => {
                    let name = name.trim();
                    let soul = soul.join(" ").trim().to_string();
                    if name.is_empty() || soul.is_empty() {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    }
                    self.push_log(
                        UiLevel::Info,
                        format!("updating llm prompt profile '{name}'"),
                    );
                    CommandOutcome::Llm(LlmCommandRequest::PromptSet {
                        name: name.to_string(),
                        soul,
                    })
                }
                [_, "remove", name] => {
                    let name = name.trim();
                    if name.is_empty() {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    }
                    self.push_log(
                        UiLevel::Info,
                        format!("removing llm prompt profile '{name}'"),
                    );
                    CommandOutcome::Llm(LlmCommandRequest::PromptRemove(name.to_string()))
                }
                [_, "preview"] => {
                    self.push_log(UiLevel::Info, "previewing active prompt profile...");
                    CommandOutcome::Llm(LlmCommandRequest::PromptPreview {
                        user_prompt: String::new(),
                    })
                }
                [_, "preview", user_prompt @ ..] => {
                    let user_prompt = user_prompt.join(" ").trim().to_string();
                    self.push_log(UiLevel::Info, "previewing active prompt profile...");
                    CommandOutcome::Llm(LlmCommandRequest::PromptPreview { user_prompt })
                }
                _ => {
                    self.show_llm_usage();
                    CommandOutcome::None
                }
            },
            _ => {
                self.show_llm_usage();
                CommandOutcome::None
            }
        }
    }

    #[cfg(test)]
    pub(super) fn log_window_bounds(&self) -> (usize, usize) {
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

    pub(super) fn clear_completion_state(&mut self) {
        self.completion_state = None;
    }

    pub(super) fn command_completion_candidates(&self, prefix: &str) -> Vec<String> {
        let mut candidates: Vec<String> = TUI_COMMANDS
            .iter()
            .filter(|command| command.starts_with(prefix))
            .map(|command| (*command).to_string())
            .collect();

        if let Some(plugin_sdk) = self.plugin_sdk.as_ref() {
            let plugin_commands = plugin_sdk.list_tui_commands();
            for command in plugin_commands {
                if !command.enabled || !command.name.starts_with(prefix) {
                    continue;
                }
                if candidates.iter().any(|existing| existing == &command.name) {
                    continue;
                }
                candidates.push(command.name);
            }
            candidates.sort();
        }

        candidates
    }

    pub(super) fn log_completion_candidates(prefix: &str) -> Vec<String> {
        LOG_SUBCOMMANDS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| format!("/log {candidate}"))
            .collect()
    }

    pub(super) fn whitelist_subcommand_candidates(prefix: &str) -> Vec<String> {
        WHITELIST_SUBCOMMANDS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| {
                if *candidate == "list" {
                    "/whitelist list".to_string()
                } else {
                    format!("/whitelist {candidate} ")
                }
            })
            .collect()
    }

    pub(super) fn whitelist_scope_candidates(verb: &str, prefix: &str) -> Vec<String> {
        WHITELIST_SCOPE_HINTS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| format!("/whitelist {verb} {candidate} "))
            .collect()
    }

    pub(super) fn llm_subcommand_candidates(prefix: &str) -> Vec<String> {
        LLM_SUBCOMMANDS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| match *candidate {
                "provider" => "/llm provider ".to_string(),
                "prompt" => "/llm prompt ".to_string(),
                _ => format!("/llm {candidate} "),
            })
            .collect()
    }

    pub(super) fn llm_prompt_subcommand_candidates(prefix: &str) -> Vec<String> {
        ["list", "use", "set", "remove", "preview"]
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| match *candidate {
                "list" => "/llm prompt list".to_string(),
                _ => format!("/llm prompt {candidate} "),
            })
            .collect()
    }

    pub(super) fn llm_provider_candidates(verb: &str, prefix: &str) -> Vec<String> {
        LLM_PROVIDER_HINTS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| format!("/llm {verb} {candidate}"))
            .collect()
    }

    pub(super) fn llm_provider_subcommand_candidates(prefix: &str) -> Vec<String> {
        ["add", "remove", "use", "list"]
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| match *candidate {
                "list" => "/llm provider list".to_string(),
                _ => format!("/llm provider {candidate} "),
            })
            .collect()
    }

    pub(super) fn llm_provider_url_candidates(action: &str, prefix: &str) -> Vec<String> {
        ["https://api.openai.com", "https://tokenflux.dev/v1"]
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| format!("/llm provider {action} {candidate}"))
            .collect()
    }

    pub(super) fn resume_completion_candidates(&self, prefix: &str) -> Vec<String> {
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

    pub(super) fn whitelist_completion_context(
        &self,
        input: &str,
    ) -> Option<(String, CompletionMode, Vec<String>)> {
        let rest = input.strip_prefix("/whitelist ")?;
        let rest = rest.trim_start();
        if rest.is_empty() {
            let candidates = Self::whitelist_subcommand_candidates("");
            return Some((
                "whitelist:subcommand:".to_string(),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let trailing_space = input.ends_with(' ');

        if tokens.len() == 1 {
            let verb = tokens[0];
            if trailing_space && matches!(verb, "add" | "remove") {
                let candidates = Self::whitelist_scope_candidates(verb, "");
                return Some((
                    format!("whitelist:{verb}:scope:"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            let candidates = Self::whitelist_subcommand_candidates(verb);
            return Some((
                format!("whitelist:subcommand:{verb}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        if tokens.len() == 2 && matches!(tokens[0], "add" | "remove") {
            let verb = tokens[0];
            let scope_prefix = if trailing_space { "" } else { tokens[1] };
            if scope_prefix.contains(':') {
                return None;
            }
            let candidates = Self::whitelist_scope_candidates(verb, scope_prefix);
            return Some((
                format!("whitelist:{verb}:scope:{scope_prefix}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        None
    }

    pub(super) fn log_completion_context(
        &self,
        input: &str,
    ) -> Option<(String, CompletionMode, Vec<String>)> {
        let rest = input.strip_prefix("/log ")?;
        let rest = rest.trim_start();
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        if tokens.len() > 1 {
            return None;
        }
        let trailing_space = input.ends_with(' ');
        let prefix = if trailing_space {
            ""
        } else {
            tokens.first().copied().unwrap_or("")
        };
        let candidates = Self::log_completion_candidates(prefix);
        Some((
            format!("log:mode:{prefix}"),
            CompletionMode::Rendered,
            candidates,
        ))
    }

    pub(super) fn llm_completion_context(
        &self,
        input: &str,
    ) -> Option<(String, CompletionMode, Vec<String>)> {
        let rest = input.strip_prefix("/llm ")?;
        let rest = rest.trim_start();
        if rest.is_empty() {
            let candidates = Self::llm_subcommand_candidates("");
            return Some((
                "llm:subcommand:".to_string(),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let trailing_space = input.ends_with(' ');
        if tokens.len() == 1 {
            let verb = tokens[0];
            if trailing_space && matches!(verb, "enable" | "on" | "disable" | "off") {
                let candidates = Self::llm_provider_candidates(verb, "");
                return Some((
                    format!("llm:{verb}:provider:"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            if trailing_space && verb == "provider" {
                let mut candidates = Self::llm_provider_subcommand_candidates("");
                candidates.extend(Self::llm_provider_candidates("provider", ""));
                return Some((
                    "llm:provider:verb:".to_string(),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            if trailing_space && verb == "prompt" {
                let candidates = Self::llm_prompt_subcommand_candidates("");
                return Some((
                    "llm:prompt:subcommand:".to_string(),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            let candidates = Self::llm_subcommand_candidates(verb);
            return Some((
                format!("llm:subcommand:{verb}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        if tokens.len() == 2 && matches!(tokens[0], "enable" | "on" | "disable" | "off") {
            let verb = tokens[0];
            let prefix = if trailing_space { "" } else { tokens[1] };
            let candidates = Self::llm_provider_candidates(verb, prefix);
            return Some((
                format!("llm:{verb}:provider:{prefix}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        if tokens.len() == 2 && tokens[0] == "provider" {
            if trailing_space && matches!(tokens[1], "add" | "remove" | "use") {
                let action = tokens[1];
                let candidates = Self::llm_provider_url_candidates(action, "");
                return Some((
                    format!("llm:provider:{action}:url:"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            let prefix = if trailing_space { "" } else { tokens[1] };
            let mut candidates = Self::llm_provider_subcommand_candidates(prefix);
            candidates.extend(Self::llm_provider_candidates("provider", prefix));
            return Some((
                format!("llm:provider:verb:{prefix}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        if tokens.len() == 3
            && tokens[0] == "provider"
            && matches!(tokens[1], "add" | "remove" | "use")
        {
            let action = tokens[1];
            let prefix = if trailing_space { "" } else { tokens[2] };
            let candidates = Self::llm_provider_url_candidates(action, prefix);
            return Some((
                format!("llm:provider:{action}:url:{prefix}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        if tokens.len() == 2 && tokens[0] == "prompt" {
            let prefix = if trailing_space { "" } else { tokens[1] };
            let candidates = Self::llm_prompt_subcommand_candidates(prefix);
            return Some((
                format!("llm:prompt:subcommand:{prefix}"),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        None
    }

    pub(super) fn completion_context(&self) -> Option<(String, CompletionMode, Vec<String>)> {
        let input = self.console_input.trim_start();
        if let Some(ctx) = self.whitelist_completion_context(input) {
            return Some(ctx);
        }
        if let Some(ctx) = self.log_completion_context(input) {
            return Some(ctx);
        }
        if let Some(ctx) = self.llm_completion_context(input) {
            return Some(ctx);
        }
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

    pub(super) fn apply_completion_candidate(mode: CompletionMode, candidate: &str) -> String {
        match mode {
            CompletionMode::Command => {
                if matches!(candidate, "/resume" | "/whitelist" | "/llm" | "/ask") {
                    format!("{candidate} ")
                } else {
                    candidate.to_string()
                }
            }
            CompletionMode::ResumeUid => format!("/resume {candidate}"),
            CompletionMode::Rendered => candidate.to_string(),
        }
    }

    pub(super) fn autocomplete_console_input(&mut self) {
        if let Some(state) = self.completion_state.clone() {
            if state.mode == CompletionMode::Command && state.candidates.len() == 1 {
                let rendered =
                    Self::apply_completion_candidate(state.mode.clone(), &state.candidates[0]);
                if self.console_input == rendered && rendered.ends_with(' ') {
                    self.clear_completion_state();
                }
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

    pub(super) fn completion_preview(&self) -> Option<String> {
        if !self.console_input.trim_start().starts_with('/') {
            return None;
        }
        let (_, mode, candidates) = self.completion_context()?;
        let candidate = candidates.first()?;
        Some(Self::apply_completion_candidate(mode, candidate))
    }

    pub(super) fn completion_preview_suffix(&self) -> Option<String> {
        let preview = self.completion_preview()?;
        if preview == self.console_input {
            return None;
        }
        if let Some(suffix) = preview.strip_prefix(self.console_input.as_str()) {
            if suffix.is_empty() {
                None
            } else {
                Some(suffix.to_string())
            }
        } else {
            Some(format!("  ({preview})"))
        }
    }

    pub(super) fn show_resume_list(&mut self) {
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

    pub(super) fn switch_resume(&mut self, uid: &str) -> Result<(), String> {
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

    pub(super) fn handle_console_command(&mut self, command: &str) -> CommandOutcome {
        let cmd = command.trim();
        let mut parts = cmd.split_whitespace();
        let command_name = parts.next().unwrap_or_default();
        let is_builtin = TUI_COMMANDS.contains(&command_name);
        if is_builtin
            && self
                .plugin_sdk
                .as_ref()
                .is_some_and(|sdk| sdk.is_builtin_tui_command_disabled(command_name))
        {
            self.push_log(
                UiLevel::Warn,
                format!("command '{}' disabled by plugin policy", command_name),
            );
            return CommandOutcome::None;
        }
        match command_name {
            "/quit" | "/exit" => {
                self.push_log(UiLevel::Warn, "shutdown requested by console command");
                CommandOutcome::Quit
            }
            "/help" => {
                self.push_log(
                    UiLevel::Info,
                    "commands: /help /reload /log [on|off] /clear /adapters /ask <prompt> /resumes /history /resume <uid> /llm ... /whitelist ... /quit /exit",
                );
                self.push_log(
                    UiLevel::Info,
                    "whitelist: /whitelist list | /whitelist add <id|scope:id|scope id> | /whitelist remove <id|scope:id|scope id>",
                );
                self.push_log(
                    UiLevel::Info,
                    "llm: /llm model <name> | /llm apikey <k1> [k2 ...] | /llm provider [name] | /llm provider add|remove|use <base-url> | /llm provider list | /llm on|off|enable|disable [provider] | /llm prompt list|use <name>|set <name> <soul>|remove <name>|preview [user_prompt]",
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
                if let Some(plugin_sdk) = self.plugin_sdk.as_ref() {
                    let plugin_commands = plugin_sdk
                        .list_tui_commands()
                        .into_iter()
                        .filter(|entry| entry.enabled)
                        .collect::<Vec<_>>();
                    if !plugin_commands.is_empty() {
                        self.push_log(
                            UiLevel::Info,
                            format!("plugin commands ({}):", plugin_commands.len()),
                        );
                        for command in plugin_commands {
                            self.push_log(
                                UiLevel::Info,
                                format!("  {} - {}", command.name, command.description),
                            );
                        }
                    }
                }
                CommandOutcome::None
            }
            "/reload" => {
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, "usage: /reload");
                    return CommandOutcome::None;
                }
                self.push_log(UiLevel::Info, "reload requested");
                CommandOutcome::Reload
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
                    return CommandOutcome::None;
                };
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, "usage: /log [on|off]");
                    return CommandOutcome::None;
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
                CommandOutcome::None
            }
            "/clear" => {
                self.logs.clear();
                self.scroll_logs_bottom();
                self.push_log(UiLevel::Info, "console cleared");
                CommandOutcome::None
            }
            "/adapters" => {
                if self.adapters.is_empty() {
                    self.push_log(UiLevel::Info, "no adapters configured");
                    return CommandOutcome::None;
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
                CommandOutcome::None
            }
            "/ask" => {
                let prompt = parts.collect::<Vec<&str>>().join(" ").trim().to_string();
                if prompt.is_empty() {
                    self.push_log(UiLevel::Warn, "usage: /ask <prompt>");
                    return CommandOutcome::None;
                }
                self.push_log(
                    UiLevel::Info,
                    "sending /ask request in background (ui remains responsive)...",
                );
                CommandOutcome::Ask(prompt)
            }
            "/resumes" | "/history" => {
                self.show_resume_list();
                CommandOutcome::None
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
                    return CommandOutcome::None;
                };
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, "usage: /resume <uid>");
                    return CommandOutcome::None;
                }
                if let Err(err) = self.switch_resume(uid) {
                    self.push_log(UiLevel::Warn, err);
                }
                CommandOutcome::None
            }
            "/whitelist" => {
                let args: Vec<&str> = parts.collect();
                self.handle_whitelist_command(&args)
            }
            "/llm" => {
                let args: Vec<&str> = parts.collect();
                self.handle_llm_command(&args)
            }
            _ => {
                if let Some(plugin_sdk) = self.plugin_sdk.as_ref() {
                    if let Some(plugin_command) = plugin_sdk.get_tui_command(command_name) {
                        if !plugin_command.enabled {
                            self.push_log(
                                UiLevel::Warn,
                                format!("plugin command '{}' is disabled", plugin_command.name),
                            );
                            return CommandOutcome::None;
                        }
                        let args = parts.map(ToString::to_string).collect::<Vec<_>>();
                        return CommandOutcome::PluginCommand {
                            command: plugin_command.name,
                            args,
                        };
                    }
                }
                self.push_log(UiLevel::Warn, format!("unknown command: {cmd}. try /help"));
                CommandOutcome::None
            }
        }
    }

    pub(super) fn command_help_text(&self) -> String {
        let input = self.console_input.trim();
        if input.is_empty() {
            return "命令说明: 输入 /help 查看命令；Tab 自动补全；Enter 执行".to_string();
        }

        if !input.starts_with('/') {
            return "命令说明: 普通文本不会执行命令，请以 / 开头；例如 /ask 你好".to_string();
        }

        if let Some(help) = Self::command_help_for_line(input) {
            return help.to_string();
        }
        if let Some(help) = self.plugin_command_help_for_line(input) {
            return help;
        }

        if let Some((_, mode, candidates)) = self.completion_context()
            && let Some(candidate) = candidates.first()
        {
            let rendered = Self::apply_completion_candidate(mode, candidate);
            if let Some(help) = Self::command_help_for_line(rendered.as_str()) {
                return help.to_string();
            }
            if let Some(help) = self.plugin_command_help_for_line(rendered.as_str()) {
                return help;
            }
        }

        "命令说明: 未知命令，输入 /help 查看可用命令".to_string()
    }

    pub(super) fn command_help_for_line(line: &str) -> Option<&'static str> {
        let mut parts = line.split_whitespace();
        let command = parts.next()?;

        match command {
            "/help" => Some("命令说明: /help 显示全部命令与快捷键"),
            "/reload" => Some("命令说明: /reload 重新加载配置与适配器状态"),
            "/log" => Some("命令说明: /log [on|off] 切换日志控制台视图"),
            "/clear" => Some("命令说明: /clear 清空当前日志窗口"),
            "/adapters" => Some("命令说明: /adapters 列出适配器连接状态与端点"),
            "/ask" => Some("命令说明: /ask <prompt> 后台请求 LLM，不阻塞终端刷新"),
            "/resumes" | "/history" => Some("命令说明: /resumes 或 /history 查看历史会话快照"),
            "/resume" => Some("命令说明: /resume <uid> 切换到指定历史会话"),
            "/llm" => Some("命令说明: /llm 管理模型、Key、provider 与 prompt profile"),
            "/whitelist" => Some("命令说明: /whitelist 管理 external /help 白名单"),
            "/quit" | "/exit" => Some("命令说明: /quit 或 /exit 安全退出程序"),
            _ => None,
        }
    }

    fn plugin_command_help_for_line(&self, line: &str) -> Option<String> {
        let command = line.split_whitespace().next()?;
        let plugin_sdk = self.plugin_sdk.as_ref()?;
        let entry = plugin_sdk.get_tui_command(command)?;
        if entry.enabled {
            Some(format!(
                "命令说明: {} 来自插件 {}，{}",
                entry.name, entry.plugin_id, entry.description
            ))
        } else {
            Some(format!(
                "命令说明: {} 来自插件 {}，当前已禁用",
                entry.name, entry.plugin_id
            ))
        }
    }
}
