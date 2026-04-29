use super::*;
use crate::i18n::{tr, trf};
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

fn resume_session_with_log(uid: &str, message: String) -> ResumeSession {
    ResumeSession {
        uid: uid.to_string(),
        created_at: "2026-04-21T00:00:00+08:00".to_string(),
        updated_at: "2026-04-21T00:00:00+08:00".to_string(),
        logs: vec![UiLog {
            level: UiLevel::Info,
            timestamp: "00:00:00".to_string(),
            message,
        }],
        command_history: vec![],
    }
}

fn temp_plugin_dir(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.push(format!("rsliteyuki-plugin-{name}-{nanos}"));
    path
}

fn register_test_manifest_plugin(
    id: &str,
    name: &str,
    runtime_kind: &str,
) -> (PluginManager, PathBuf) {
    let manager = PluginManager::new();
    let plugin_dir = temp_plugin_dir(id);
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{
  "id": "{id}",
  "name": "{name}",
  "type": "service",
  "runtime": {{
    "kind": "{runtime_kind}",
    "entrypoint": "demo:bootstrap"
  }}
}}"#
        ),
    )
    .expect("plugin manifest should be written");

    manager
        .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
        .expect("manifest discovery should succeed");

    (manager, plugin_dir)
}

fn register_test_manifest_plugins(specs: &[(&str, &str, &str)]) -> (PluginManager, Vec<PathBuf>) {
    let manager = PluginManager::new();
    let mut plugin_dirs = Vec::new();
    for (id, name, runtime_kind) in specs {
        let plugin_dir = temp_plugin_dir(id);
        std::fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
        std::fs::write(
            plugin_dir.join("plugin.json"),
            format!(
                r#"{{
  "id": "{id}",
  "name": "{name}",
  "type": "service",
  "runtime": {{
    "kind": "{runtime_kind}",
    "entrypoint": "demo:bootstrap"
  }}
}}"#
            ),
        )
        .expect("plugin manifest should be written");
        manager
            .discover_manifest_plugins_in_dirs([plugin_dir.as_path()])
            .expect("manifest discovery should succeed");
        plugin_dirs.push(plugin_dir);
    }

    (manager, plugin_dirs)
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
fn autocomplete_commands_scope_hints() {
    let path = temp_resume_path("autocomplete-commands");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    app.console_input = "/commands ".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/commands tui");

    remove_file_if_exists(&path);
}

#[test]
fn autocomplete_commands_management_hints() {
    let path = temp_resume_path("autocomplete-commands-manage");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.plugin_sdk = Some(PluginSdk::default());

    app.console_input = "/commands d".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/commands disable ");

    app.console_input = "/commands disable ".to_string();
    app.clear_completion_state();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/commands disable tui ");

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
fn max_log_scroll_accounts_for_wrapped_visual_lines() {
    let path = temp_resume_path("log-visual-scroll");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.set_log_text_width(24);
    app.set_log_view_rows(2);
    app.push_log(UiLevel::Info, "0123456789 abcdefghij klmnopqrst uvwxyz");

    assert!(
        app.max_log_scroll() > 0,
        "wrapped log should produce scrollable visual lines"
    );

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
    let usage = tr("tui.command.reload_usage");
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains(usage.as_str()))
    );

    remove_file_if_exists(&path);
}

#[test]
fn apply_reload_result_updates_shared_llm_command_prefix() {
    let path = temp_resume_path("reload-llm-command-prefix");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    let shared = Arc::new(RwLock::new("/ask".to_string()));
    app.bind_llm_command_prefix(shared.clone());

    app.apply_reload_result(ReloadResult {
        adapters: Vec::new(),
        adapter_autostart: false,
        tui_config: test_tui_config(path.clone()),
        locale: crate::i18n::AppLocale::ZhCn,
        help_whitelist: Vec::new(),
        llm_command_prefix: "/qa".to_string(),
        disabled_commands: Vec::new(),
        disabled_plugins: vec!["builtin-liteecho".to_string()],
        warnings: Vec::new(),
    });

    let prefix = shared
        .read()
        .expect("llm command prefix lock should be readable in test")
        .clone();
    assert_eq!(prefix, "/qa");
    assert!(app.disabled_plugins.contains("builtin-liteecho"));
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains("external LLM command prefix: /qa"))
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

    let entries_title = trf("tui.whitelist.entries", &[("count", "2")]);
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains(entries_title.as_str()))
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

    let outcome = app.handle_console_command("/llm provider list");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::ListProviderUrls)
    ));

    let outcome = app.handle_console_command("/llm provider add https://api.openai.com");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::AddProviderUrl(_))
    ));

    let outcome = app.handle_console_command("/llm provider remove https://api.openai.com");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::RemoveProviderUrl(_))
    ));

    let outcome = app.handle_console_command("/llm provider use https://api.openai.com");
    assert!(matches!(
        outcome,
        CommandOutcome::Llm(LlmCommandRequest::UseProviderUrl(_))
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
    let usage = tr("tui.command.ask_usage");
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains(usage.as_str()))
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
fn tui_builtin_completion_excludes_adapter_only_commands() {
    let path = temp_resume_path("autocomplete-scope-filter");
    remove_file_if_exists(&path);

    let app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );

    let candidates = app.command_completion_candidates("/");
    assert!(candidates.iter().any(|candidate| candidate == "/help"));
    assert!(!candidates.iter().any(|candidate| candidate == "/su"));

    remove_file_if_exists(&path);
}

#[test]
fn commands_command_lists_scope_filtered_catalog() {
    let path = temp_resume_path("command-catalog");
    remove_file_if_exists(&path);

    let mut adapter_scope_app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    adapter_scope_app.handle_console_command("/commands adapter:onebot11");
    let adapter_title = trf(
        "tui.command.catalog.title",
        &[("scope", "adapter:onebot11")],
    );
    assert!(
        adapter_scope_app
            .logs
            .iter()
            .any(|log| log.message.contains(adapter_title.as_str()))
    );
    assert!(
        adapter_scope_app
            .logs
            .iter()
            .any(|log| log.message.contains("/su <password>"))
    );
    assert!(
        adapter_scope_app
            .logs
            .iter()
            .all(|log| !log.message.contains("/reload"))
    );

    let mut tui_scope_app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    tui_scope_app.handle_console_command("/commands tui");
    let tui_title = trf("tui.command.catalog.title", &[("scope", "tui")]);
    assert!(
        tui_scope_app
            .logs
            .iter()
            .any(|log| log.message.contains(tui_title.as_str()))
    );
    assert!(
        tui_scope_app
            .logs
            .iter()
            .any(|log| log.message.contains("/reload"))
    );
    assert!(
        tui_scope_app
            .logs
            .iter()
            .all(|log| !log.message.contains("/su <password>"))
    );

    remove_file_if_exists(&path);
}

#[test]
fn commands_command_can_disable_and_enable_builtin_scope_command() {
    let path = temp_resume_path("command-manage-builtin");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.plugin_sdk = Some(PluginSdk::default());

    app.handle_console_command("/commands disable tui /help");
    let disabled_message = trf(
        "tui.command.changed",
        &[
            ("command", "/help"),
            ("state", tr("tui.command.state.disabled").as_str()),
            ("scope", "tui"),
            ("targets", tr("tui.command.target.builtin").as_str()),
        ],
    );
    assert!(
        app.logs
            .iter()
            .any(|log| { log.message.contains(disabled_message.as_str()) })
    );
    assert!(
        !app.command_completion_candidates("/")
            .contains(&"/help".to_string())
    );
    assert_eq!(
        app.command_help_for_line("/help").as_deref(),
        Some("命令说明: /help 当前已禁用")
    );

    app.handle_console_command("/help");
    let policy_message = trf("tui.command.disabled_by_policy", &[("command", "/help")]);
    assert!(
        app.logs
            .iter()
            .any(|log| { log.message.contains(policy_message.as_str()) })
    );

    app.handle_console_command("/commands enable tui /help");
    let enabled_message = trf(
        "tui.command.changed",
        &[
            ("command", "/help"),
            ("state", tr("tui.command.state.enabled").as_str()),
            ("scope", "tui"),
            ("targets", tr("tui.command.target.builtin").as_str()),
        ],
    );
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains(enabled_message.as_str()))
    );
    assert!(
        app.command_completion_candidates("/")
            .contains(&"/help".to_string())
    );

    remove_file_if_exists(&path);
}

#[test]
fn plugins_command_lists_catalog_with_runtime_type() {
    let path = temp_resume_path("plugin-catalog");
    remove_file_if_exists(&path);
    let (manager, plugin_dir) =
        register_test_manifest_plugin("builtin-liteecho", "Builtin LiteEcho", "python");

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.bind_plugin_manager(manager);

    app.handle_console_command("/plugins");
    let catalog_title = trf("plugin.catalog.title", &[("count", "1")]);
    let service_tag = tr("plugin.type.service");
    let enabled_tag = tr("plugin.catalog.tag.enabled");
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains(catalog_title.as_str()))
    );
    assert!(app.logs.iter().any(|log| {
        log.message.contains("builtin-liteecho (python)")
            && log.message.contains(service_tag.as_str())
            && log.message.contains(enabled_tag.as_str())
    }));

    let _ = std::fs::remove_dir_all(plugin_dir);
    remove_file_if_exists(&path);
}

#[test]
fn plugins_command_can_disable_and_enable_plugin() {
    let path = temp_resume_path("plugin-manage");
    remove_file_if_exists(&path);
    let (manager, plugin_dir) =
        register_test_manifest_plugin("builtin-liteecho", "Builtin LiteEcho", "python");

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.bind_plugin_manager(manager);

    let disable = app.handle_console_command("/plugins disable builtin-liteecho");
    assert!(matches!(
        disable,
        CommandOutcome::PersistDisabledPlugins { .. }
    ));
    assert!(app.disabled_plugins.contains("builtin-liteecho"));
    let disabled_message = trf(
        "plugin.command.changed",
        &[
            ("plugin", "builtin-liteecho"),
            ("state", tr("plugin.catalog.tag.disabled").as_str()),
            ("runtime", "python"),
        ],
    );
    assert!(
        app.logs
            .iter()
            .any(|log| { log.message.contains(disabled_message.as_str()) })
    );

    let enable = app.handle_console_command("/plugins enable builtin-liteecho");
    assert!(matches!(
        enable,
        CommandOutcome::PersistDisabledPlugins { .. }
    ));
    assert!(!app.disabled_plugins.contains("builtin-liteecho"));
    let enabled_message = trf(
        "plugin.command.changed",
        &[
            ("plugin", "builtin-liteecho"),
            ("state", tr("plugin.catalog.tag.enabled").as_str()),
            ("runtime", "python"),
        ],
    );
    assert!(
        app.logs
            .iter()
            .any(|log| { log.message.contains(enabled_message.as_str()) })
    );

    let _ = std::fs::remove_dir_all(plugin_dir);
    remove_file_if_exists(&path);
}

#[test]
fn dashboard_plugin_selection_moves_and_wraps() {
    let path = temp_resume_path("dashboard-plugin-selection");
    remove_file_if_exists(&path);
    let (manager, plugin_dirs) = register_test_manifest_plugins(&[
        ("zeta-plugin", "Zeta Plugin", "python"),
        ("alpha-plugin", "Alpha Plugin", "python"),
        ("beta-plugin", "Beta Plugin", "python"),
    ]);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.bind_plugin_manager(manager);

    assert!(app.is_dashboard_view());
    assert_eq!(app.normalized_dashboard_plugin_index(3), Some(0));

    assert!(app.move_dashboard_plugin_selection(1));
    assert!(app.is_dashboard_plugins_focus());
    assert_eq!(app.normalized_dashboard_plugin_index(3), Some(1));

    assert!(app.move_dashboard_plugin_selection(1));
    assert_eq!(app.normalized_dashboard_plugin_index(3), Some(2));

    assert!(app.move_dashboard_plugin_selection(1));
    assert_eq!(app.normalized_dashboard_plugin_index(3), Some(0));

    assert!(app.move_dashboard_plugin_selection(-1));
    assert_eq!(app.normalized_dashboard_plugin_index(3), Some(2));

    for plugin_dir in plugin_dirs {
        let _ = std::fs::remove_dir_all(plugin_dir);
    }
    remove_file_if_exists(&path);
}

#[test]
fn dashboard_plugin_toggle_matches_plugins_command_semantics() {
    let path = temp_resume_path("dashboard-plugin-toggle");
    remove_file_if_exists(&path);
    let (manager, plugin_dirs) =
        register_test_manifest_plugins(&[("alpha-plugin", "Alpha Plugin", "python")]);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.bind_plugin_manager(manager);

    app.move_dashboard_plugin_selection(1);
    assert_eq!(
        app.command_help_text(),
        "插件面板: 当前 alpha-plugin；Up/Down 选择；Enter 禁用；Tab 回到命令"
    );
    let disable_command = app
        .dashboard_toggle_selected_plugin_command()
        .expect("dashboard toggle should produce a disable command");
    assert_eq!(disable_command, "/plugins disable alpha-plugin");

    let disable = app.handle_console_command(disable_command.as_str());
    assert!(matches!(
        disable,
        CommandOutcome::PersistDisabledPlugins {
            ref entries,
            ref rollback_entries,
        } if entries == &vec!["alpha-plugin".to_string()] && rollback_entries.is_empty()
    ));
    assert!(app.disabled_plugins.contains("alpha-plugin"));
    assert_eq!(
        app.command_help_text(),
        "插件面板: 当前 alpha-plugin；Up/Down 选择；Enter 启用；Tab 回到命令"
    );

    let enable_command = app
        .dashboard_toggle_selected_plugin_command()
        .expect("dashboard toggle should produce an enable command");
    assert_eq!(enable_command, "/plugins enable alpha-plugin");

    let enable = app.handle_console_command(enable_command.as_str());
    assert!(matches!(
        enable,
        CommandOutcome::PersistDisabledPlugins {
            ref entries,
            ref rollback_entries,
        } if entries.is_empty() && rollback_entries == &vec!["alpha-plugin".to_string()]
    ));
    assert!(!app.disabled_plugins.contains("alpha-plugin"));

    for plugin_dir in plugin_dirs {
        let _ = std::fs::remove_dir_all(plugin_dir);
    }
    remove_file_if_exists(&path);
}

#[test]
fn empty_input_help_text_switches_to_dashboard_plugin_panel_hint() {
    let path = temp_resume_path("dashboard-plugin-help-hint");
    remove_file_if_exists(&path);
    let (manager, plugin_dirs) =
        register_test_manifest_plugins(&[("alpha-plugin", "Alpha Plugin", "python")]);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    app.bind_plugin_manager(manager);

    assert_eq!(
        app.command_help_text(),
        "命令面板: 输入 /help 查看命令；Tab 切到插件；Enter 执行；PgUp/PgDn/Home/End 滚动日志"
    );

    app.cycle_dashboard_focus();
    assert!(app.is_dashboard_plugin_panel_active());
    assert_eq!(
        app.command_help_text(),
        "插件面板: 当前 alpha-plugin；Up/Down 选择；Enter 禁用；Tab 回到命令"
    );

    for plugin_dir in plugin_dirs {
        let _ = std::fs::remove_dir_all(plugin_dir);
    }
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
    assert_eq!(redacted, "/llm apikey <已隐藏:2>");

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

    app.console_input = "/llm provider ".to_string();
    app.autocomplete_console_input();
    assert_eq!(app.console_input, "/llm provider add ");

    app.clear_completion_state();
    app.console_input = "/llm provider add ".to_string();
    app.autocomplete_console_input();
    assert_eq!(
        app.console_input,
        "/llm provider add https://api.openai.com"
    );

    app.clear_completion_state();
    app.console_input = "/llm provider use ".to_string();
    app.autocomplete_console_input();
    assert_eq!(
        app.console_input,
        "/llm provider use https://api.openai.com"
    );

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
    app.resume_store
        .sessions
        .push(resume_session_with_log("old-large-a", large.clone()));
    app.resume_store
        .sessions
        .push(resume_session_with_log("old-large-b", large));

    app.enforce_resume_limits();
    app.flush_resume_if_needed(true);

    let written_size = std::fs::metadata(&path)
        .expect("resume store should be written")
        .len() as usize;
    assert!(app.resume_store.persisted_size_bytes() <= app.resume_max_size_bytes);
    assert!(written_size <= app.resume_max_size_bytes);
    assert!(
        app.resume_store
            .sessions
            .iter()
            .any(|session| session.uid == active_uid)
    );

    remove_file_if_exists(&path);
}

#[test]
fn resume_size_limit_drops_frontmost_old_resume() {
    let path = temp_resume_path("resume-size-order");
    remove_file_if_exists(&path);

    let mut app = AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        test_tui_config(path.clone()),
    );
    let active_uid = app.active_resume_uid().to_string();

    app.resume_store
        .sessions
        .push(resume_session_with_log("old-1", "a".repeat(512)));
    app.resume_store
        .sessions
        .push(resume_session_with_log("old-2", "b".repeat(512)));

    let mut store_after_one_trim = app.resume_store.clone();
    store_after_one_trim.sessions.remove(1);
    store_after_one_trim.update_session(&active_uid, &app.logs, &app.command_history);
    let max_size_bytes = store_after_one_trim.persisted_size_bytes();
    assert!(app.resume_store.persisted_size_bytes() > max_size_bytes);

    app.resume_max_size_bytes = max_size_bytes;
    app.flush_resume_if_needed(true);

    let remaining_uids = app
        .resume_store
        .sessions
        .iter()
        .map(|session| session.uid.as_str())
        .collect::<Vec<_>>();
    let written_size = std::fs::metadata(&path)
        .expect("resume store should be written")
        .len() as usize;

    assert!(remaining_uids.contains(&active_uid.as_str()));
    assert!(!remaining_uids.contains(&"old-1"));
    assert!(remaining_uids.contains(&"old-2"));
    assert!(app.resume_store.persisted_size_bytes() <= app.resume_max_size_bytes);
    assert!(written_size <= app.resume_max_size_bytes);

    remove_file_if_exists(&path);
}
