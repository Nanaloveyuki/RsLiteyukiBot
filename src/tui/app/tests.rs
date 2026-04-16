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

fn test_tui_config(path: PathBuf) -> TuiConfig {
    TuiConfig {
        resume_store_path: path,
        resume_max_sessions: 64,
        resume_max_size_mib: 16,
    }
}

#[test]
fn command_history_navigation_restores_draft() {
    let path = temp_resume_path("history-navigation");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
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
        test_tui_config(path.clone()),
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
fn completion_preview_shows_suffix_for_best_match() {
    let path = temp_resume_path("completion-preview");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    app.console_input = "/re".to_string();
    assert_eq!(app.completion_preview_suffix().as_deref(), Some("load"));

    app.console_input = "/help".to_string();
    assert!(app.completion_preview_suffix().is_none());

    app.resume_store
        .create_session("resume-preview-001".to_string());
    app.console_input = "/resume resume-pr".to_string();
    assert_eq!(
        app.completion_preview_suffix().as_deref(),
        Some("eview-001")
    );

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
        test_tui_config(path.clone()),
    );

    app.console_input = "/".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/help");

    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/reload");

    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/log");

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
        test_tui_config(path.clone()),
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
fn autocomplete_whitelist_subcommands_and_scope() {
    let path = temp_resume_path("autocomplete-whitelist");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    app.console_input = "/whitelist ".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/whitelist add ");

    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/whitelist remove ");

    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/whitelist list");

    app.console_input = "/whitelist add ".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/whitelist add private ");

    remove_file_if_exists(&path);
}

#[test]
fn autocomplete_log_subcommands() {
    let path = temp_resume_path("autocomplete-log");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    app.console_input = "/log ".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/log on");

    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/log off");

    remove_file_if_exists(&path);
}

#[test]
fn command_cursor_position_tracks_input_width() {
    let area = Rect::new(0, 0, 20, 3);
    let (x, y) = command_cursor_position(area, "/help");
    assert_eq!(y, 1);
    assert_eq!(x, 1 + 2 + 5);

    let (x_cjk, y_cjk) = command_cursor_position(area, "你好");
    assert_eq!(y_cjk, 1);
    assert_eq!(x_cjk, 1 + 2 + 4);
}

#[test]
fn wrap_text_hard_respects_display_width_for_cjk() {
    let wrapped = wrap_text_hard("你好世界", 4);
    assert_eq!(wrapped, vec!["你好".to_string(), "世界".to_string()]);

    let wrapped_mixed = wrap_text_hard("ab你好cd", 4);
    assert_eq!(wrapped_mixed, vec!["ab你".to_string(), "好cd".to_string()]);
}

#[test]
fn command_help_text_shows_ask_non_blocking_hint() {
    let path = temp_resume_path("command-help-ask");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.console_input = "/ask hello".to_string();
    let help = app.command_help_text();
    assert!(help.contains("/ask <prompt>"));
    assert!(help.contains("不阻塞终端刷新"));

    remove_file_if_exists(&path);
}

#[test]
fn command_help_text_uses_completion_for_prefix() {
    let path = temp_resume_path("command-help-prefix");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.console_input = "/as".to_string();
    let help = app.command_help_text();
    assert!(help.contains("/ask <prompt>"));

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
        test_tui_config(path.clone()),
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
        test_tui_config(path.clone()),
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
        test_tui_config(path.clone()),
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

#[test]
fn reload_command_is_recognized() {
    let path = temp_resume_path("reload-command");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    let outcome = app.handle_console_command("/reload");
    assert!(matches!(outcome, CommandOutcome::Reload));

    let outcome = app.handle_console_command("/reload now");
    assert!(matches!(outcome, CommandOutcome::None));
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains("usage: /reload"))
    );

    remove_file_if_exists(&path);
}

#[test]
fn whitelist_command_can_add_remove_and_list_entries() {
    let path = temp_resume_path("whitelist-command");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    let shared = Arc::new(RwLock::new(HashSet::new()));
    app.bind_help_whitelist(shared.clone());

    let outcome = app.handle_console_command("/whitelist add private 3541766758");
    assert!(matches!(outcome, CommandOutcome::PersistWhitelist(_)));
    let outcome = app.handle_console_command("/whitelist add gourp:699493240");
    assert!(matches!(outcome, CommandOutcome::PersistWhitelist(_)));
    app.handle_console_command("/whitelist list");

    let lock = shared
        .read()
        .expect("whitelist lock should be readable in test");
    assert!(lock.contains("private:3541766758"));
    assert!(lock.contains("group:699493240"));
    drop(lock);

    let outcome = app.handle_console_command("/whitelist remove group:699493240");
    assert!(matches!(outcome, CommandOutcome::PersistWhitelist(_)));
    let lock = shared
        .read()
        .expect("whitelist lock should be readable in test");
    assert!(!lock.contains("group:699493240"));
    assert!(lock.contains("private:3541766758"));
    drop(lock);

    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains("whitelist entries"))
    );

    remove_file_if_exists(&path);
}

#[test]
fn llm_command_parses_actions() {
    let path = temp_resume_path("llm-command");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    let outcome = app.handle_console_command("/llm model gpt-4.1-mini");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::SetModel(_))
    ));

    let outcome = app.handle_console_command("/llm apikey k1 k2");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::AddApiKeys(_))
    ));

    let outcome = app.handle_console_command("/llm provider openai");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::ProbeProvider(Some(_)))
    ));

    let outcome = app.handle_console_command("/llm on openai");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::SetEnabled { enabled: true, .. })
    ));

    let outcome = app.handle_console_command("/llm prompt list");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::PromptList)
    ));

    let outcome = app.handle_console_command("/llm prompt use default");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::PromptUse(_))
    ));

    let outcome = app.handle_console_command("/llm prompt set roleplay answer like pirate");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::PromptSet { .. })
    ));

    let outcome = app.handle_console_command("/llm prompt preview hello");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::PromptPreview { .. })
    ));

    remove_file_if_exists(&path);
}

#[test]
fn ask_command_parses_prompt() {
    let path = temp_resume_path("ask-command");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    let outcome = app.handle_console_command("/ask hello world");
    assert!(matches!(outcome, CommandOutcome::Ask(_)));

    let outcome = app.handle_console_command("/ask");
    assert!(matches!(outcome, CommandOutcome::None));
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains("usage: /ask"))
    );

    remove_file_if_exists(&path);
}

#[test]
fn autocomplete_includes_ask_command() {
    let path = temp_resume_path("autocomplete-ask");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    app.console_input = "/as".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/ask ");

    remove_file_if_exists(&path);
}

#[test]
fn llm_apikey_command_is_redacted_for_display() {
    let path = temp_resume_path("redact-apikey");
    remove_file_if_exists(&path);

    let app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    let redacted = app.redact_console_command_for_display("/llm apikey sk-1 sk-2");
    assert_eq!(redacted, "/llm apikey <redacted:2>");

    remove_file_if_exists(&path);
}

#[test]
fn llm_apikey_command_is_not_recorded_into_history() {
    let path = temp_resume_path("history-apikey");
    remove_file_if_exists(&path);

    let app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    assert!(!app.should_record_command_history("/llm apikey sk-secret"));
    assert!(app.should_record_command_history("/ask hello"));

    remove_file_if_exists(&path);
}

#[test]
fn autocomplete_llm_subcommands_and_provider() {
    let path = temp_resume_path("autocomplete-llm");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    app.console_input = "/llm ".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/llm model ");

    app.console_input = "/llm on ".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/llm on openai");

    app.console_input = "/llm prompt ".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/llm prompt list");

    remove_file_if_exists(&path);
}

#[test]
fn resume_count_limit_keeps_active_session() {
    let path = temp_resume_path("resume-count-limit");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        TuiConfig {
            resume_store_path: path.clone(),
            resume_max_sessions: 2,
            resume_max_size_mib: 16,
        },
    );
    let active_uid = app.active_resume_uid().to_string();

    app.resume_store.create_session("old-1".to_string());
    app.resume_store.create_session("old-2".to_string());
    app.resume_store.create_session("old-3".to_string());
    app.enforce_resume_limits();

    assert!(app.resume_store.sessions.len() <= 2);
    assert!(
        app.resume_store
            .sessions
            .iter()
            .any(|session| session.uid == active_uid)
    );

    remove_file_if_exists(&path);
}

#[test]
fn resume_size_limit_keeps_active_session() {
    let path = temp_resume_path("resume-size-limit");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        TuiConfig {
            resume_store_path: path.clone(),
            resume_max_sessions: 64,
            resume_max_size_mib: 1,
        },
    );
    let active_uid = app.active_resume_uid().to_string();

    let large = "x".repeat(600 * 1024);
    app.resume_store.sessions.push(ResumeSession {
        uid: "old-large-a".to_string(),
        created_at: Local::now().to_rfc3339(),
        updated_at: Local::now().to_rfc3339(),
        logs: vec![UiLog {
            level: UiLevel::Info,
            timestamp: "00:00:00".to_string(),
            message: large.clone(),
        }],
        command_history: vec![],
    });
    app.resume_store.sessions.push(ResumeSession {
        uid: "old-large-b".to_string(),
        created_at: Local::now().to_rfc3339(),
        updated_at: Local::now().to_rfc3339(),
        logs: vec![UiLog {
            level: UiLevel::Info,
            timestamp: "00:00:00".to_string(),
            message: large,
        }],
        command_history: vec![],
    });

    app.enforce_resume_limits();

    assert!(app.resume_store.estimated_size_bytes() <= app.resume_max_size_bytes);
    assert!(
        app.resume_store
            .sessions
            .iter()
            .any(|session| session.uid == active_uid)
    );

    remove_file_if_exists(&path);
}
