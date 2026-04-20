#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AdapterProtocol {
    OneBot11,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CommandScope {
    All,
    Tui,
    Adapter(AdapterProtocol),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BuiltinCommandId {
    Help,
    Reload,
    Log,
    Clear,
    Adapters,
    Commands,
    Plugins,
    Ask,
    Resumes,
    History,
    Resume,
    Llm,
    Whitelist,
    Quit,
    Exit,
    Su,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CommandNameOverrides<'a> {
    pub onebot_ask_prefix: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BuiltinCommandSpec {
    pub id: BuiltinCommandId,
    pub summary: &'static str,
    pub detail: &'static str,
    pub usage_hint: Option<&'static str>,
    pub accepts_arguments: bool,
    pub completion_trailing_space: bool,
    pub scopes: &'static [CommandScope],
    pub tui_name: Option<&'static str>,
    pub onebot_name: Option<&'static str>,
    pub onebot_aliases: &'static [&'static str],
}

const NO_ALIASES: [&str; 0] = [];
const HELP_ALIASES: [&str; 1] = ["help"];
const SU_ALIASES: [&str; 1] = ["su"];
const SCOPE_TUI: [CommandScope; 1] = [CommandScope::Tui];
const SCOPE_ONEBOT: [CommandScope; 1] = [CommandScope::Adapter(AdapterProtocol::OneBot11)];
const SCOPE_TUI_ONEBOT: [CommandScope; 2] = [
    CommandScope::Tui,
    CommandScope::Adapter(AdapterProtocol::OneBot11),
];

const BUILTIN_COMMANDS: [BuiltinCommandSpec; 16] = [
    BuiltinCommandSpec {
        id: BuiltinCommandId::Help,
        summary: "显示当前作用域可用命令",
        detail: "显示当前作用域可用命令",
        usage_hint: None,
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI_ONEBOT,
        tui_name: Some("/help"),
        onebot_name: Some("/help"),
        onebot_aliases: &HELP_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Reload,
        summary: "重新加载配置与适配器状态",
        detail: "重新加载配置与适配器状态",
        usage_hint: None,
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI,
        tui_name: Some("/reload"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Log,
        summary: "切换日志控制台视图",
        detail: "切换日志控制台视图",
        usage_hint: Some("[on|off]"),
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI,
        tui_name: Some("/log"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Clear,
        summary: "清空当前日志窗口",
        detail: "清空当前日志窗口",
        usage_hint: None,
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI,
        tui_name: Some("/clear"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Adapters,
        summary: "列出适配器连接状态与端点",
        detail: "列出适配器连接状态与端点",
        usage_hint: None,
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI,
        tui_name: Some("/adapters"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Commands,
        summary: "按 scope 查看或管理 builtin/plugin 命令",
        detail: "按 scope 查看命令清单，或通过 enable/disable 管理命令启用状态",
        usage_hint: Some("[scope] | enable <scope> <name> | disable <scope> <name>"),
        accepts_arguments: false,
        completion_trailing_space: true,
        scopes: &SCOPE_TUI,
        tui_name: Some("/commands"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Plugins,
        summary: "查看或管理插件启用状态",
        detail: "列出插件目录中的插件，并通过 enable/disable 开关插件后自动 reload",
        usage_hint: Some("[list] | enable <plugin-id> | disable <plugin-id>"),
        accepts_arguments: false,
        completion_trailing_space: true,
        scopes: &SCOPE_TUI,
        tui_name: Some("/plugins"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Ask,
        summary: "请求 LLM 生成回复",
        detail: "后台请求 LLM，不阻塞终端刷新",
        usage_hint: Some("<prompt>"),
        accepts_arguments: true,
        completion_trailing_space: true,
        scopes: &SCOPE_TUI_ONEBOT,
        tui_name: Some("/ask"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Resumes,
        summary: "查看历史会话快照",
        detail: "查看历史会话快照",
        usage_hint: None,
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI,
        tui_name: Some("/resumes"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::History,
        summary: "查看历史会话快照",
        detail: "查看历史会话快照",
        usage_hint: None,
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI,
        tui_name: Some("/history"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Resume,
        summary: "切换到指定历史会话",
        detail: "切换到指定历史会话",
        usage_hint: Some("<uid>"),
        accepts_arguments: true,
        completion_trailing_space: true,
        scopes: &SCOPE_TUI,
        tui_name: Some("/resume"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Llm,
        summary: "管理模型、Key、provider 与 prompt profile",
        detail: "管理模型、Key、provider 与 prompt profile",
        usage_hint: Some("..."),
        accepts_arguments: false,
        completion_trailing_space: true,
        scopes: &SCOPE_TUI,
        tui_name: Some("/llm"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Whitelist,
        summary: "管理 external /help 白名单",
        detail: "管理 external /help 白名单",
        usage_hint: Some("..."),
        accepts_arguments: false,
        completion_trailing_space: true,
        scopes: &SCOPE_TUI,
        tui_name: Some("/whitelist"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Quit,
        summary: "安全退出程序",
        detail: "安全退出程序",
        usage_hint: None,
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI,
        tui_name: Some("/quit"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Exit,
        summary: "安全退出程序",
        detail: "安全退出程序",
        usage_hint: None,
        accepts_arguments: false,
        completion_trailing_space: false,
        scopes: &SCOPE_TUI,
        tui_name: Some("/exit"),
        onebot_name: None,
        onebot_aliases: &NO_ALIASES,
    },
    BuiltinCommandSpec {
        id: BuiltinCommandId::Su,
        summary: "认证当前会话为 superuser",
        detail: "认证当前会话为 superuser",
        usage_hint: Some("<password>"),
        accepts_arguments: true,
        completion_trailing_space: false,
        scopes: &SCOPE_ONEBOT,
        tui_name: None,
        onebot_name: Some("/su"),
        onebot_aliases: &SU_ALIASES,
    },
];

impl BuiltinCommandSpec {
    pub(crate) fn supports_scope(&self, scope: CommandScope) -> bool {
        self.scopes
            .iter()
            .any(|entry| *entry == CommandScope::All || *entry == scope)
    }
}

pub(crate) fn parse_command_argument(message: &str, command_prefix: &str) -> Option<String> {
    let command_prefix = command_prefix.trim();
    if command_prefix.is_empty() {
        return None;
    }

    let message = message.trim();
    if message == command_prefix {
        return Some(String::new());
    }

    let remainder = message.strip_prefix(command_prefix)?;
    let mut chars = remainder.chars();
    if !chars.next().is_some_and(char::is_whitespace) {
        return None;
    }

    Some(remainder.trim().to_string())
}

pub(crate) fn builtin_commands_for_scope(
    scope: CommandScope,
) -> impl Iterator<Item = &'static BuiltinCommandSpec> {
    BUILTIN_COMMANDS
        .iter()
        .filter(move |command| command.supports_scope(scope))
}

pub(crate) fn command_primary_name(
    command: &BuiltinCommandSpec,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Option<String> {
    match scope {
        CommandScope::Tui => command.tui_name.map(ToString::to_string),
        CommandScope::Adapter(AdapterProtocol::OneBot11) => {
            if command.id == BuiltinCommandId::Ask {
                return Some(normalize_onebot_ask_prefix(overrides.onebot_ask_prefix));
            }
            command.onebot_name.map(ToString::to_string)
        }
        CommandScope::All => None,
    }
}

pub(crate) fn command_usage_label(
    command: &BuiltinCommandSpec,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Option<String> {
    let mut label = command_primary_name(command, scope, overrides)?;
    if let Some(hint) = command.usage_hint {
        label.push(' ');
        label.push_str(hint);
    }
    Some(label)
}

pub(crate) fn parse_command_scope_token(raw: &str) -> Option<CommandScope> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let compact = trimmed.to_ascii_lowercase().replace([' ', '_', '-'], "");
    match compact.as_str() {
        "all" => Some(CommandScope::All),
        "tui" => Some(CommandScope::Tui),
        "adapter:onebot11" | "adapter:onebotv11" | "adapteronebot11" | "onebot11" | "onebotv11" => {
            Some(CommandScope::Adapter(AdapterProtocol::OneBot11))
        }
        _ => None,
    }
}

pub(crate) fn builtin_command_names(
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Vec<String> {
    builtin_commands_for_scope(scope)
        .filter_map(|command| command_primary_name(command, scope, overrides))
        .collect()
}

pub(crate) fn is_builtin_command_name(
    name: &str,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> bool {
    find_builtin_command_by_name(name, scope, overrides).is_some()
}

pub(crate) fn command_completion_trailing_space(
    name: &str,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> bool {
    find_builtin_command_by_name(name, scope, overrides)
        .map(|command| command.completion_trailing_space)
        .unwrap_or(false)
}

pub(crate) fn command_help_text_for_name(
    name: &str,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Option<String> {
    let command = find_builtin_command_by_name(name, scope, overrides)?;
    let label = command_usage_label(command, scope, overrides)?;
    Some(format!("命令说明: {label} {}", command.detail))
}

pub(crate) fn normalize_builtin_command_name_for_scope(
    name: &str,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Option<String> {
    let command = find_builtin_command_by_name(name, scope, overrides)?;
    command_primary_name(command, scope, overrides)
}

pub(crate) fn render_builtin_help_lines_filtered<F>(
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
    mut include_command: F,
) -> Vec<String>
where
    F: FnMut(&BuiltinCommandSpec, &str) -> bool,
{
    let mut lines = vec![format!("可用命令 ({}):", scope_label(scope))];
    for command in builtin_commands_for_scope(scope) {
        let Some(label) = command_usage_label(command, scope, overrides) else {
            continue;
        };
        let Some(name) = command_primary_name(command, scope, overrides) else {
            continue;
        };
        if !include_command(command, name.as_str()) {
            continue;
        }
        lines.push(format!("{label} - {}", command.summary));
    }

    match scope {
        CommandScope::Tui => {
            lines.push(
                "快捷键: Up/Down history, Tab cycle-complete, PgUp/PgDn/Home/End scroll logs"
                    .to_string(),
            );
            lines.push("日志视图: 空输入 + Up/Down 按行滚动日志".to_string());
            lines.push(
                "说明: adapter 外部命令会按 scope 过滤，例如 /su 不会出现在 TUI 中".to_string(),
            );
        }
        CommandScope::Adapter(AdapterProtocol::OneBot11) => {
            lines.push("说明: /reload /log 等控制台管理命令仅在 TUI 可用".to_string());
            lines.push("说明: 外部 /help 与外部 LLM 命令需先通过 /su 完成认证".to_string());
            lines.push("说明: OneBot 的 /su 仅允许私聊发送".to_string());
        }
        CommandScope::All => {}
    }

    lines
}

pub(crate) fn render_builtin_help_lines(
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Vec<String> {
    render_builtin_help_lines_filtered(scope, overrides, |_, _| true)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_builtin_help_text(
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> String {
    render_builtin_help_lines(scope, overrides).join("\n")
}

pub(crate) fn matches_builtin_command_message(
    command_id: BuiltinCommandId,
    message: &str,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> bool {
    command_argument_for_message(command_id, message, scope, overrides).is_some()
}

pub(crate) fn command_argument_for_message(
    command_id: BuiltinCommandId,
    message: &str,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Option<String> {
    let command = BUILTIN_COMMANDS
        .iter()
        .find(|entry| entry.id == command_id && entry.supports_scope(scope))?;
    let message = message.trim();

    if command.accepts_arguments {
        for name in command_names_for_matching(command, scope, overrides) {
            if let Some(argument) = parse_command_argument(message, name.as_str()) {
                return Some(argument);
            }
        }
        return None;
    }

    command_names_for_matching(command, scope, overrides)
        .into_iter()
        .find(|name| name == message)
        .map(|_| String::new())
}

fn find_builtin_command_by_name(
    name: &str,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Option<&'static BuiltinCommandSpec> {
    let name = name.trim();
    BUILTIN_COMMANDS.iter().find(|command| {
        command.supports_scope(scope)
            && command_names_for_matching(command, scope, overrides)
                .into_iter()
                .any(|candidate| candidate == name)
    })
}

fn command_names_for_matching(
    command: &BuiltinCommandSpec,
    scope: CommandScope,
    overrides: CommandNameOverrides<'_>,
) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(primary) = command_primary_name(command, scope, overrides) {
        names.push(primary);
    }
    if matches!(scope, CommandScope::Adapter(AdapterProtocol::OneBot11)) {
        names.extend(
            command
                .onebot_aliases
                .iter()
                .map(|alias| (*alias).to_string()),
        );
    }
    names
}

fn normalize_onebot_ask_prefix(prefix: Option<&str>) -> String {
    let normalized = prefix.unwrap_or("/ask").trim();
    if normalized.is_empty() {
        "/ask".to_string()
    } else {
        normalized.to_string()
    }
}

pub(crate) fn command_scope_label(scope: CommandScope) -> &'static str {
    match scope {
        CommandScope::All => "all",
        CommandScope::Tui => "tui",
        CommandScope::Adapter(AdapterProtocol::OneBot11) => "adapter:onebot11",
    }
}

fn scope_label(scope: CommandScope) -> &'static str {
    command_scope_label(scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
    fn tui_scope_excludes_adapter_only_commands() {
        let commands = builtin_command_names(CommandScope::Tui, CommandNameOverrides::default());
        assert!(commands.iter().any(|command| command == "/help"));
        assert!(!commands.iter().any(|command| command == "/su"));
    }

    #[test]
    fn onebot_help_text_excludes_tui_only_commands() {
        let help_text = render_builtin_help_text(
            CommandScope::Adapter(AdapterProtocol::OneBot11),
            CommandNameOverrides::default(),
        );
        assert!(help_text.contains("/help"));
        assert!(help_text.contains("/su <password>"));
        assert!(!help_text.contains("/reload -"));
    }
}
