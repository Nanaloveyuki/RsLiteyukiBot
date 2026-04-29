use super::*;

#[test]
// 必要测试
fn onebot_scope_uses_dynamic_ask_prefix() {
    let argument = command_argument_for_message(
        BuiltinCommandId::Ask,
        "/qa hello world",
        CommandScope::Adapter(AdapterProtocol::OneBot11),
        CommandNameOverrides {
            onebot_ask_prefix: Some("/qa"),
        },
    );
    assert_eq!(argument.as_deref(), Some("hello world"));
}

#[test]
// 必要测试
fn tui_scope_excludes_adapter_only_commands() {
    let commands = builtin_command_names(CommandScope::Tui, CommandNameOverrides::default());
    assert!(commands.iter().any(|command| command == "/help"));
    assert!(!commands.iter().any(|command| command == "/su"));
}

#[test]
// 必要测试
fn onebot_help_text_excludes_tui_only_commands() {
    let help_text = render_builtin_help_text(
        CommandScope::Adapter(AdapterProtocol::OneBot11),
        CommandNameOverrides::default(),
    );
    assert!(help_text.contains("/help"));
    assert!(help_text.contains("/su <password>"));
    assert!(!help_text.contains("/reload -"));
}
