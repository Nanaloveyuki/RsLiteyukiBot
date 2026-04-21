use super::*;
use crate::command_registry::{
    AdapterProtocol, CommandNameOverrides, CommandScope, builtin_command_names,
    builtin_commands_for_scope, command_completion_trailing_space, command_help_text_for_name,
    command_primary_name, command_scope_label, command_usage_label, is_builtin_command_name,
    normalize_builtin_command_name_for_scope, parse_command_scope_token,
    render_builtin_help_lines_filtered,
};
use crate::i18n::{tr, trf};
use liteyukibot_core::{PluginCatalogEntry, PluginLoadState, PluginRuntimeKind, PluginType};

enum CommandsAction<'a> {
    List(Option<CommandScope>),
    SetEnabled {
        enabled: bool,
        scope: CommandScope,
        name: &'a str,
    },
}

enum PluginsAction<'a> {
    List,
    SetEnabled { enabled: bool, plugin_id: &'a str },
}

impl AppState {
    pub(super) fn bind_help_whitelist(&mut self, whitelist: Arc<RwLock<HashSet<String>>>) {
        self.help_whitelist = Some(whitelist);
    }

    pub(super) fn bind_llm_command_prefix(&mut self, command_prefix: Arc<RwLock<String>>) {
        self.llm_command_prefix = Some(command_prefix);
    }

    pub(super) fn show_commands_usage(&mut self) {
        self.push_log(UiLevel::Warn, tr("tui.commands.usage"));
    }

    pub(super) fn command_scope_candidates(prefix: &str) -> Vec<String> {
        COMMAND_SCOPE_HINTS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| format!("/commands {candidate}"))
            .collect()
    }

    pub(super) fn command_root_candidates(prefix: &str) -> Vec<String> {
        let mut candidates = Self::command_scope_candidates(prefix);
        candidates.extend(
            COMMAND_MANAGEMENT_VERBS
                .iter()
                .filter(|candidate| candidate.starts_with(prefix))
                .map(|candidate| format!("/commands {candidate} ")),
        );
        candidates
    }

    pub(super) fn show_plugins_usage(&mut self) {
        self.push_log(UiLevel::Warn, tr("tui.plugins.usage"));
    }

    fn plugins_subcommand_candidates(prefix: &str) -> Vec<String> {
        PLUGIN_MANAGEMENT_VERBS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| {
                if *candidate == "list" {
                    "/plugins list".to_string()
                } else {
                    format!("/plugins {candidate} ")
                }
            })
            .collect()
    }

    fn parse_plugins_action<'a>(args: &'a [&'a str]) -> Result<PluginsAction<'a>, String> {
        match args {
            [] | ["list"] => Ok(PluginsAction::List),
            ["enable", plugin_id] => Ok(PluginsAction::SetEnabled {
                enabled: true,
                plugin_id,
            }),
            ["disable", plugin_id] => Ok(PluginsAction::SetEnabled {
                enabled: false,
                plugin_id,
            }),
            _ => Err("usage".to_string()),
        }
    }

    fn command_scope_candidates_for_action(action: &str, prefix: &str) -> Vec<String> {
        COMMAND_SCOPE_HINTS
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .filter(|candidate| **candidate != "all")
            .map(|candidate| format!("/commands {action} {candidate} "))
            .collect()
    }

    pub(super) fn parse_commands_scope(args: &[&str]) -> Result<Option<CommandScope>, String> {
        match args {
            [] => Ok(None),
            [scope] => parse_command_scope_token(scope)
                .map(Some)
                .ok_or_else(|| "scope should be tui|adapter:onebot11|onebot11|all".to_string()),
            ["adapter", protocol] => {
                parse_command_scope_token(format!("adapter:{protocol}").as_str())
                    .map(Some)
                    .ok_or_else(|| "scope should be tui|adapter:onebot11|onebot11|all".to_string())
            }
            _ => Err("scope should be tui|adapter:onebot11|onebot11|all".to_string()),
        }
    }

    fn parse_manageable_commands_scope(args: &[&str]) -> Result<CommandScope, String> {
        let scope = Self::parse_commands_scope(args)?
            .ok_or_else(|| "scope should be tui|adapter:onebot11|onebot11".to_string())?;
        if matches!(scope, CommandScope::All) {
            return Err("scope should be tui|adapter:onebot11|onebot11".to_string());
        }
        Ok(scope)
    }

    fn parse_commands_action<'a>(args: &'a [&'a str]) -> Result<CommandsAction<'a>, String> {
        match args {
            ["enable", scope, name] => Ok(CommandsAction::SetEnabled {
                enabled: true,
                scope: Self::parse_manageable_commands_scope(&[*scope])?,
                name,
            }),
            ["enable", "adapter", protocol, name] => Ok(CommandsAction::SetEnabled {
                enabled: true,
                scope: Self::parse_manageable_commands_scope(&["adapter", *protocol])?,
                name,
            }),
            ["disable", scope, name] => Ok(CommandsAction::SetEnabled {
                enabled: false,
                scope: Self::parse_manageable_commands_scope(&[*scope])?,
                name,
            }),
            ["disable", "adapter", protocol, name] => Ok(CommandsAction::SetEnabled {
                enabled: false,
                scope: Self::parse_manageable_commands_scope(&["adapter", *protocol])?,
                name,
            }),
            _ => Ok(CommandsAction::List(Self::parse_commands_scope(args)?)),
        }
    }

    fn command_catalog_scopes(scope: Option<CommandScope>) -> Vec<CommandScope> {
        match scope {
            None | Some(CommandScope::All) => vec![
                CommandScope::Tui,
                CommandScope::Adapter(AdapterProtocol::OneBot11),
            ],
            Some(scope) => vec![scope],
        }
    }

    fn current_onebot_command_prefix(&self) -> String {
        self.llm_command_prefix
            .as_ref()
            .and_then(|shared| shared.read().ok().map(|value| value.trim().to_string()))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/ask".to_string())
    }

    fn normalize_user_command_name(raw: &str) -> Option<String> {
        let first = raw.split_whitespace().next()?.trim();
        if first.is_empty() {
            return None;
        }
        if first.starts_with('/') {
            Some(first.to_ascii_lowercase())
        } else {
            Some(format!("/{}", first.to_ascii_lowercase()))
        }
    }

    fn is_builtin_scope_command_enabled(&self, scope: CommandScope, command_name: &str) -> bool {
        !self.plugin_sdk.as_ref().is_some_and(|sdk| {
            sdk.is_builtin_command_disabled(command_scope_label(scope), command_name)
        })
    }

    fn scope_command_name_candidates(
        &self,
        scope: CommandScope,
        prefix: &str,
        enabled: Option<bool>,
    ) -> Vec<String> {
        let onebot_prefix = self.current_onebot_command_prefix();
        let overrides = CommandNameOverrides {
            onebot_ask_prefix: Some(onebot_prefix.as_str()),
        };
        let mut candidates: Vec<String> = builtin_commands_for_scope(scope)
            .filter_map(|command| command_primary_name(command, scope, overrides))
            .filter(|command| command.starts_with(prefix))
            .filter(|command| {
                enabled.is_none_or(|enabled| {
                    self.is_builtin_scope_command_enabled(scope, command.as_str()) == enabled
                })
            })
            .collect();

        if let Some(plugin_sdk) = self.plugin_sdk.as_ref() {
            for command in plugin_sdk.list_scope_commands(command_scope_label(scope)) {
                if !command.name.starts_with(prefix) {
                    continue;
                }
                if enabled.is_some_and(|enabled| command.enabled != enabled) {
                    continue;
                }
                if candidates.iter().any(|existing| existing == &command.name) {
                    continue;
                }
                candidates.push(command.name);
            }
        }

        candidates.sort();
        candidates
    }

    fn command_catalog_lines(&self, selected_scope: Option<CommandScope>) -> Vec<String> {
        let plugin_sdk = self.plugin_sdk.as_ref();
        let mut lines = Vec::new();
        let onebot_prefix = self.current_onebot_command_prefix();

        for scope in Self::command_catalog_scopes(selected_scope) {
            let overrides = CommandNameOverrides {
                onebot_ask_prefix: Some(onebot_prefix.as_str()),
            };
            lines.push(trf(
                "tui.command.catalog.title",
                &[("scope", command_scope_label(scope))],
            ));

            for command in builtin_commands_for_scope(scope) {
                let Some(label) = command_usage_label(command, scope, overrides) else {
                    continue;
                };
                let enabled = command_primary_name(command, scope, overrides)
                    .map(|name| self.is_builtin_scope_command_enabled(scope, name.as_str()))
                    .unwrap_or(true);
                let state = if enabled {
                    tr("tui.command.catalog.tag.enabled")
                } else {
                    tr("tui.command.catalog.tag.disabled")
                };
                lines.push(trf(
                    "tui.command.catalog.entry_builtin",
                    &[
                        ("label", label.as_str()),
                        ("summary", command.summary),
                        ("state", state.as_str()),
                    ],
                ));
            }

            if let Some(plugin_sdk) = plugin_sdk {
                for command in plugin_sdk.list_scope_commands(command_scope_label(scope)) {
                    let description = tr(command.description.as_str());
                    let mut tags = vec![
                        trf(
                            "tui.command.catalog.tag.plugin",
                            &[("plugin", command.plugin_id.as_str())],
                        ),
                        if command.enabled {
                            tr("tui.command.catalog.tag.enabled")
                        } else {
                            tr("tui.command.catalog.tag.disabled")
                        },
                    ];
                    if matches!(scope, CommandScope::Tui) {
                        tags.push(if command.executable_in_tui {
                            tr("tui.command.catalog.tag.executable")
                        } else {
                            tr("tui.command.catalog.tag.declared_only")
                        });
                    }
                    let tags_text = tags.join(", ");
                    lines.push(trf(
                        "tui.command.catalog.entry_plugin",
                        &[
                            ("name", command.name.as_str()),
                            ("description", description.as_str()),
                            ("tags", tags_text.as_str()),
                        ],
                    ));
                }
            }
        }

        lines
    }

    pub(super) fn plugin_catalog_entries(&self) -> Vec<PluginCatalogEntry> {
        self.plugin_manager
            .as_ref()
            .map(|manager| manager.plugin_catalog())
            .unwrap_or_default()
    }

    pub(super) fn plugin_runtime_label(kind: PluginRuntimeKind) -> String {
        match kind {
            PluginRuntimeKind::Native => tr("plugin.runtime.native"),
            PluginRuntimeKind::Python => tr("plugin.runtime.python"),
            PluginRuntimeKind::Lua => tr("plugin.runtime.lua"),
            PluginRuntimeKind::External => tr("plugin.runtime.external"),
        }
    }

    pub(super) fn plugin_type_label(kind: PluginType) -> String {
        match kind {
            PluginType::Application => tr("plugin.type.application"),
            PluginType::Service => tr("plugin.type.service"),
            PluginType::Module => tr("plugin.type.module"),
            PluginType::Unclassified => tr("plugin.type.unclassified"),
            PluginType::Test => tr("plugin.type.test"),
        }
    }

    pub(super) fn is_plugin_enabled(&self, plugin_id: &str) -> bool {
        !self.disabled_plugins.contains(plugin_id)
    }

    pub(super) fn selected_dashboard_plugin_entry(&self) -> Option<PluginCatalogEntry> {
        let catalog = self.plugin_catalog_entries();
        let selected_index = self.normalized_dashboard_plugin_index(catalog.len())?;
        catalog.into_iter().nth(selected_index)
    }

    pub(super) fn dashboard_plugin_summary_counts(&self) -> (usize, usize, usize) {
        let catalog = self.plugin_catalog_entries();
        let total = catalog.len();
        let enabled = catalog
            .iter()
            .filter(|entry| self.is_plugin_enabled(entry.descriptor.metadata.id.as_str()))
            .count();
        let loaded = catalog.iter().filter(|entry| entry.loaded).count();
        (total, enabled, loaded)
    }

    pub(super) fn move_dashboard_plugin_selection(&mut self, delta: isize) -> bool {
        let catalog_len = self.plugin_catalog_entries().len();
        let Some(current) = self.normalized_dashboard_plugin_index(catalog_len) else {
            self.dashboard_plugin_index = 0;
            return false;
        };
        let next = if delta < 0 {
            if current == 0 {
                catalog_len - 1
            } else {
                current - 1
            }
        } else if delta > 0 {
            (current + 1) % catalog_len
        } else {
            current
        };
        self.dashboard_plugin_index = next;
        self.dashboard_focus = DashboardFocus::Plugins;
        self.reset_history_navigation();
        next != current
    }

    pub(super) fn dashboard_toggle_selected_plugin_command(&self) -> Option<String> {
        let entry = self.selected_dashboard_plugin_entry()?;
        let plugin_id = entry.descriptor.metadata.id;
        let action = if self.is_plugin_enabled(plugin_id.as_str()) {
            "disable"
        } else {
            "enable"
        };
        Some(format!("/plugins {action} {plugin_id}"))
    }

    fn plugin_catalog_lines(&self) -> Vec<String> {
        let catalog = self.plugin_catalog_entries();
        if catalog.is_empty() {
            return vec![tr("plugin.catalog.empty")];
        }

        let mut lines = vec![trf(
            "plugin.catalog.title",
            &[("count", catalog.len().to_string().as_str())],
        )];
        for entry in catalog {
            let plugin_id = entry.descriptor.metadata.id.clone();
            let enabled = self.is_plugin_enabled(plugin_id.as_str());
            let mut tags = vec![
                Self::plugin_type_label(entry.descriptor.metadata.plugin_type),
                if enabled {
                    tr("plugin.catalog.tag.enabled")
                } else {
                    tr("plugin.catalog.tag.disabled")
                },
                if entry.loaded {
                    tr("plugin.catalog.tag.loaded")
                } else {
                    tr("plugin.catalog.tag.unloaded")
                },
                if entry.descriptor.manifest_path.is_some() {
                    tr("plugin.catalog.tag.manifest")
                } else {
                    tr("plugin.catalog.tag.registered")
                },
            ];
            if let Some(state) = entry.load_state {
                tags.push(match state {
                    PluginLoadState::Ready => tr("plugin.state.ready"),
                    PluginLoadState::Deferred => tr("plugin.state.deferred"),
                });
            }
            let runtime_label = Self::plugin_runtime_label(entry.descriptor.runtime.kind);
            let plugin_name = tr(entry.descriptor.metadata.name.as_str());
            lines.push(trf(
                "plugin.catalog.entry",
                &[
                    ("plugin", plugin_id.as_str()),
                    ("runtime", runtime_label.as_str()),
                    ("name", plugin_name.as_str()),
                    ("tags", tags.join(", ").as_str()),
                ],
            ));
            if let Some(reason) = entry
                .load_reason
                .as_deref()
                .filter(|reason| !reason.trim().is_empty())
            {
                lines.push(trf("plugin.catalog.reason", &[("reason", reason)]));
            }
        }

        lines
    }

    fn plugin_id_candidates(&self, prefix: &str, enabled: Option<bool>) -> Vec<String> {
        let mut candidates = self
            .plugin_catalog_entries()
            .into_iter()
            .map(|entry| entry.descriptor.metadata.id)
            .filter(|plugin_id| plugin_id.starts_with(prefix))
            .filter(|plugin_id| {
                enabled
                    .is_none_or(|expected| self.is_plugin_enabled(plugin_id.as_str()) == expected)
            })
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        candidates
    }

    fn set_plugin_enabled(&mut self, raw_plugin_id: &str, enabled: bool) -> CommandOutcome {
        let normalized = raw_plugin_id.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            self.show_plugins_usage();
            return CommandOutcome::None;
        }

        let Some(entry) = self.plugin_catalog_entries().into_iter().find(|entry| {
            entry
                .descriptor
                .metadata
                .id
                .eq_ignore_ascii_case(normalized.as_str())
        }) else {
            self.push_log(
                UiLevel::Warn,
                trf(
                    "plugin.command.not_found",
                    &[("plugin", normalized.as_str())],
                ),
            );
            return CommandOutcome::None;
        };

        let plugin_id = entry.descriptor.metadata.id;
        let runtime_label = Self::plugin_runtime_label(entry.descriptor.runtime.kind);
        let current_enabled = self.is_plugin_enabled(plugin_id.as_str());
        if current_enabled == enabled {
            self.push_log(
                UiLevel::Info,
                trf(
                    "plugin.command.already",
                    &[
                        ("plugin", plugin_id.as_str()),
                        (
                            "state",
                            if enabled {
                                tr("plugin.catalog.tag.enabled")
                            } else {
                                tr("plugin.catalog.tag.disabled")
                            }
                            .as_str(),
                        ),
                        ("runtime", runtime_label.as_str()),
                    ],
                ),
            );
            return CommandOutcome::None;
        }

        let mut rollback_entries = self.disabled_plugins.iter().cloned().collect::<Vec<_>>();
        rollback_entries.sort();
        if enabled {
            self.disabled_plugins.remove(plugin_id.as_str());
        } else {
            self.disabled_plugins.insert(plugin_id.clone());
        }
        let mut entries = self.disabled_plugins.iter().cloned().collect::<Vec<_>>();
        entries.sort();

        self.push_log(
            UiLevel::Info,
            trf(
                "plugin.command.changed",
                &[
                    ("plugin", plugin_id.as_str()),
                    (
                        "state",
                        if enabled {
                            tr("plugin.catalog.tag.enabled")
                        } else {
                            tr("plugin.catalog.tag.disabled")
                        }
                        .as_str(),
                    ),
                    ("runtime", runtime_label.as_str()),
                ],
            ),
        );
        self.push_log(UiLevel::Info, tr("plugin.reload.persisting"));

        CommandOutcome::PersistDisabledPlugins {
            entries,
            rollback_entries,
        }
    }

    fn set_scoped_command_enabled(
        &mut self,
        scope: CommandScope,
        name: &str,
        enabled: bool,
    ) -> CommandOutcome {
        let Some(plugin_sdk) = self.plugin_sdk.clone() else {
            self.push_log(UiLevel::Warn, tr("tui.command.manager_unavailable"));
            return CommandOutcome::None;
        };

        let scope_label = command_scope_label(scope);
        let onebot_prefix = self.current_onebot_command_prefix();
        let overrides = CommandNameOverrides {
            onebot_ask_prefix: Some(onebot_prefix.as_str()),
        };
        let builtin_name = normalize_builtin_command_name_for_scope(name, scope, overrides);
        let normalized_name = Self::normalize_user_command_name(name).unwrap_or_else(|| {
            name.split_whitespace()
                .next()
                .unwrap_or(name)
                .trim()
                .to_string()
        });
        let plugin_matches = plugin_sdk
            .list_scope_commands(scope_label)
            .into_iter()
            .filter(|command| command.name.eq_ignore_ascii_case(normalized_name.as_str()))
            .collect::<Vec<_>>();

        if builtin_name.is_none() && plugin_matches.is_empty() {
            self.push_log(
                UiLevel::Warn,
                trf(
                    "tui.command.not_found_in_scope",
                    &[
                        ("command", normalized_name.as_str()),
                        ("scope", scope_label),
                    ],
                ),
            );
            return CommandOutcome::None;
        }

        let builtin_changed = if let Some(command_name) = builtin_name.as_deref() {
            let current = self.is_builtin_scope_command_enabled(scope, command_name);
            if current != enabled {
                if let Err(err) =
                    plugin_sdk.set_builtin_command_enabled(scope_label, command_name, enabled)
                {
                    self.push_log(
                        UiLevel::Error,
                        trf(
                            "tui.command.update_builtin_failed",
                            &[
                                ("command", command_name),
                                ("scope", scope_label),
                                ("err", err.to_string().as_str()),
                            ],
                        ),
                    );
                    return CommandOutcome::None;
                }
                true
            } else {
                false
            }
        } else {
            false
        };

        let plugin_changed = if plugin_matches.is_empty() {
            false
        } else {
            let changed = plugin_matches
                .iter()
                .any(|command| command.enabled != enabled);
            if let Err(err) =
                plugin_sdk.set_scope_command_enabled(scope_label, normalized_name.as_str(), enabled)
            {
                self.push_log(
                    UiLevel::Error,
                    trf(
                        "tui.command.update_plugin_failed",
                        &[
                            ("command", normalized_name.as_str()),
                            ("scope", scope_label),
                            ("err", err.to_string().as_str()),
                        ],
                    ),
                );
                return CommandOutcome::None;
            }
            changed
        };

        let mut targets = Vec::new();
        if builtin_name.is_some() {
            targets.push(tr("tui.command.target.builtin"));
        }
        if !plugin_matches.is_empty() {
            targets.push(trf(
                "tui.command.target.plugin",
                &[("count", plugin_matches.len().to_string().as_str())],
            ));
        }
        let command_label = builtin_name
            .clone()
            .unwrap_or_else(|| normalized_name.clone());
        let state_label = if enabled {
            tr("tui.command.state.enabled")
        } else {
            tr("tui.command.state.disabled")
        };
        if builtin_changed || plugin_changed {
            self.push_log(
                UiLevel::Info,
                trf(
                    "tui.command.changed",
                    &[
                        ("command", command_label.as_str()),
                        ("state", state_label.as_str()),
                        ("scope", scope_label),
                        ("targets", targets.join(", ").as_str()),
                    ],
                ),
            );
        } else {
            self.push_log(
                UiLevel::Info,
                trf(
                    "tui.command.already",
                    &[
                        ("command", command_label.as_str()),
                        ("state", state_label.as_str()),
                        ("scope", scope_label),
                        ("targets", targets.join(", ").as_str()),
                    ],
                ),
            );
        }

        let entries = plugin_sdk.list_disabled_scope_commands();
        CommandOutcome::PersistDisabledCommands {
            rollback_entries: if builtin_changed || plugin_changed {
                let _ = if let Some(command_name) = builtin_name.as_deref() {
                    plugin_sdk.set_builtin_command_enabled(scope_label, command_name, !enabled)
                } else {
                    Ok(false)
                };
                if !plugin_matches.is_empty() {
                    let _ = plugin_sdk.set_scope_command_enabled(
                        scope_label,
                        normalized_name.as_str(),
                        !enabled,
                    );
                }
                let rollback = plugin_sdk.list_disabled_scope_commands();
                let _ = plugin_sdk.sync_disabled_scope_commands(&entries);
                rollback
            } else {
                entries.clone()
            },
            entries,
        }
    }

    pub(super) fn show_whitelist_usage(&mut self) {
        self.push_log(UiLevel::Warn, tr("tui.whitelist.usage"));
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
            self.push_log(UiLevel::Warn, tr("tui.whitelist.bridge_unavailable"));
            return;
        };
        let Ok(lock) = shared.read() else {
            self.push_log(UiLevel::Error, tr("tui.whitelist.lock_failed"));
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
                        app.push_log(UiLevel::Info, tr("tui.whitelist.empty"));
                        return;
                    }
                    let mut entries: Vec<String> = set.iter().cloned().collect();
                    entries.sort();
                    app.push_log(
                        UiLevel::Info,
                        trf(
                            "tui.whitelist.entries",
                            &[("count", entries.len().to_string().as_str())],
                        ),
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
                    self.push_log(UiLevel::Warn, tr("tui.whitelist.bridge_unavailable"));
                    return CommandOutcome::None;
                };
                let Ok(mut lock) = shared.write() else {
                    self.push_log(UiLevel::Error, tr("tui.whitelist.lock_failed"));
                    return CommandOutcome::None;
                };

                if subcommand == "add" {
                    if lock.insert(entry.clone()) {
                        let mut entries: Vec<String> = lock.iter().cloned().collect();
                        entries.sort();
                        self.push_log(
                            UiLevel::Info,
                            trf("tui.whitelist.added", &[("entry", entry.as_str())]),
                        );
                        self.push_log(UiLevel::Info, tr("tui.whitelist.persisting"));
                        return CommandOutcome::PersistWhitelist(entries);
                    }
                    self.push_log(
                        UiLevel::Info,
                        trf("tui.whitelist.already_exists", &[("entry", entry.as_str())]),
                    );
                    CommandOutcome::None
                } else {
                    if lock.remove(entry.as_str()) {
                        let mut entries: Vec<String> = lock.iter().cloned().collect();
                        entries.sort();
                        self.push_log(
                            UiLevel::Info,
                            trf("tui.whitelist.removed", &[("entry", entry.as_str())]),
                        );
                        self.push_log(UiLevel::Info, tr("tui.whitelist.persisting"));
                        return CommandOutcome::PersistWhitelist(entries);
                    }
                    self.push_log(
                        UiLevel::Info,
                        trf("tui.whitelist.not_found", &[("entry", entry.as_str())]),
                    );
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
        self.push_log(UiLevel::Warn, tr("tui.llm.usage"));
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
            tr("tui.llm.apikey.redacted")
        } else {
            let key_count = key_count.to_string();
            trf(
                "tui.llm.apikey.redacted_count",
                &[("count", key_count.as_str())],
            )
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
                self.push_log(
                    UiLevel::Info,
                    trf("tui.llm.model.updating", &[("model", model)]),
                );
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
                    trf(
                        "tui.llm.apikey.adding",
                        &[("count", keys.len().to_string().as_str())],
                    ),
                );
                CommandOutcome::Llm(LlmCommandRequest::AddApiKeys(keys))
            }
            "provider" => match args {
                [_, "list"] => {
                    self.push_log(UiLevel::Info, tr("tui.llm.provider.listing"));
                    CommandOutcome::Llm(LlmCommandRequest::ListProviderUrls)
                }
                [_, "add", provider_url] => {
                    let Some(provider_url) = Self::parse_llm_provider_url(provider_url) else {
                        self.show_llm_usage();
                        return CommandOutcome::None;
                    };
                    self.push_log(
                        UiLevel::Info,
                        trf(
                            "tui.llm.provider.adding",
                            &[("provider_url", provider_url.as_str())],
                        ),
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
                        trf(
                            "tui.llm.provider.removing",
                            &[("provider_url", provider_url.as_str())],
                        ),
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
                        trf(
                            "tui.llm.provider.switching",
                            &[("provider_url", provider_url.as_str())],
                        ),
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
                            trf(
                                "tui.llm.provider.probing_override",
                                &[("provider", provider)],
                            ),
                        );
                    } else {
                        self.push_log(UiLevel::Info, tr("tui.llm.provider.probing_current"));
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
                    trf(
                        "tui.llm.state.setting",
                        &[
                            (
                                "state",
                                if enabled {
                                    tr("tui.command.state.enabled")
                                } else {
                                    tr("tui.command.state.disabled")
                                }
                                .as_str(),
                            ),
                            (
                                "provider_suffix",
                                provider
                                    .as_deref()
                                    .map(|provider| {
                                        trf("tui.llm.provider_suffix", &[("provider", provider)])
                                    })
                                    .unwrap_or_default()
                                    .as_str(),
                            ),
                        ],
                    ),
                );
                CommandOutcome::Llm(LlmCommandRequest::SetEnabled { enabled, provider })
            }
            "prompt" => match args {
                [_, "list"] => {
                    self.push_log(UiLevel::Info, tr("tui.llm.prompt.listing"));
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
                        trf("tui.llm.prompt.switching", &[("name", name)]),
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
                        trf("tui.llm.prompt.updating", &[("name", name)]),
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
                        trf("tui.llm.prompt.removing", &[("name", name)]),
                    );
                    CommandOutcome::Llm(LlmCommandRequest::PromptRemove(name.to_string()))
                }
                [_, "preview"] => {
                    self.push_log(UiLevel::Info, tr("tui.llm.prompt.previewing"));
                    CommandOutcome::Llm(LlmCommandRequest::PromptPreview {
                        user_prompt: String::new(),
                    })
                }
                [_, "preview", user_prompt @ ..] => {
                    let user_prompt = user_prompt.join(" ").trim().to_string();
                    self.push_log(UiLevel::Info, tr("tui.llm.prompt.previewing"));
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
        let mut candidates: Vec<String> =
            builtin_command_names(CommandScope::Tui, CommandNameOverrides::default())
                .into_iter()
                .filter(|command| command.starts_with(prefix))
                .filter(|command| self.is_builtin_scope_command_enabled(CommandScope::Tui, command))
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

    pub(super) fn commands_completion_context(
        &self,
        input: &str,
    ) -> Option<(String, CompletionMode, Vec<String>)> {
        let raw = input.strip_prefix("/commands ")?;
        let raw = raw.trim_start();
        if raw.is_empty() {
            let candidates = Self::command_root_candidates("");
            return Some((
                "commands:root:".to_string(),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        let tokens: Vec<&str> = raw.split_whitespace().collect();
        let trailing_space = input.ends_with(' ');
        let verb = tokens.first().copied().unwrap_or_default();

        if COMMAND_MANAGEMENT_VERBS.contains(&verb) {
            if tokens.len() == 1 && !trailing_space {
                let candidates = Self::command_root_candidates(verb);
                return Some((
                    format!("commands:root:{verb}"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            if tokens.len() == 1 {
                let candidates = Self::command_scope_candidates_for_action(verb, "");
                return Some((
                    format!("commands:{verb}:scope:"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            if tokens.len() == 2 {
                let scope_prefix = if trailing_space { "" } else { tokens[1] };
                if trailing_space {
                    let scope = parse_command_scope_token(tokens[1])?;
                    if matches!(scope, CommandScope::All) {
                        return None;
                    }
                    let desired_enabled = verb == "enable";
                    let candidates = self
                        .scope_command_name_candidates(scope, "", Some(!desired_enabled))
                        .into_iter()
                        .map(|name| {
                            format!("/commands {verb} {} {name}", command_scope_label(scope))
                        })
                        .collect();
                    return Some((
                        format!("commands:{verb}:name:{}:", command_scope_label(scope)),
                        CompletionMode::Rendered,
                        candidates,
                    ));
                }
                let candidates = Self::command_scope_candidates_for_action(verb, scope_prefix);
                return Some((
                    format!("commands:{verb}:scope:{scope_prefix}"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            if tokens.len() == 3 {
                let scope = parse_command_scope_token(tokens[1])?;
                if matches!(scope, CommandScope::All) {
                    return None;
                }
                let desired_enabled = verb == "enable";
                let prefix = if trailing_space { "" } else { tokens[2] };
                let candidates = self
                    .scope_command_name_candidates(scope, prefix, Some(!desired_enabled))
                    .into_iter()
                    .map(|name| format!("/commands {verb} {} {name}", command_scope_label(scope)))
                    .collect();
                return Some((
                    format!(
                        "commands:{verb}:name:{}:{prefix}",
                        command_scope_label(scope)
                    ),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }
            return None;
        }

        if raw.chars().any(char::is_whitespace) {
            return None;
        }
        let prefix = if trailing_space { "" } else { raw };
        let candidates = Self::command_root_candidates(prefix);
        Some((
            format!("commands:root:{prefix}"),
            CompletionMode::Rendered,
            candidates,
        ))
    }

    pub(super) fn plugins_completion_context(
        &self,
        input: &str,
    ) -> Option<(String, CompletionMode, Vec<String>)> {
        let raw = input.strip_prefix("/plugins ")?;
        let raw = raw.trim_start();
        if raw.is_empty() {
            let candidates = Self::plugins_subcommand_candidates("");
            return Some((
                "plugins:root:".to_string(),
                CompletionMode::Rendered,
                candidates,
            ));
        }

        let tokens: Vec<&str> = raw.split_whitespace().collect();
        let trailing_space = input.ends_with(' ');
        let verb = tokens.first().copied().unwrap_or_default();

        if matches!(verb, "enable" | "disable") {
            if tokens.len() == 1 && !trailing_space {
                let candidates = Self::plugins_subcommand_candidates(verb);
                return Some((
                    format!("plugins:root:{verb}"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }

            if tokens.len() == 1 || tokens.len() == 2 {
                let desired_enabled = verb == "enable";
                let prefix = if trailing_space || tokens.len() == 1 {
                    ""
                } else {
                    tokens[1]
                };
                let candidates = self
                    .plugin_id_candidates(prefix, Some(!desired_enabled))
                    .into_iter()
                    .map(|plugin_id| format!("/plugins {verb} {plugin_id}"))
                    .collect::<Vec<_>>();
                return Some((
                    format!("plugins:{verb}:id:{prefix}"),
                    CompletionMode::Rendered,
                    candidates,
                ));
            }

            return None;
        }

        if raw.chars().any(char::is_whitespace) {
            return None;
        }
        let prefix = if trailing_space { "" } else { raw };
        let candidates = Self::plugins_subcommand_candidates(prefix);
        Some((
            format!("plugins:root:{prefix}"),
            CompletionMode::Rendered,
            candidates,
        ))
    }

    pub(super) fn completion_context(&self) -> Option<(String, CompletionMode, Vec<String>)> {
        let input = self.console_input.trim_start();
        if let Some(ctx) = self.commands_completion_context(input) {
            return Some(ctx);
        }
        if let Some(ctx) = self.plugins_completion_context(input) {
            return Some(ctx);
        }
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
                if command_completion_trailing_space(
                    candidate,
                    CommandScope::Tui,
                    CommandNameOverrides::default(),
                ) {
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
            self.push_log(UiLevel::Info, tr("tui.resume.empty"));
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
                let logs = session.logs.len().to_string();
                let commands = session.command_history.len().to_string();
                trf(
                    "tui.resume.entry",
                    &[
                        ("marker", marker),
                        ("uid", session.uid.as_str()),
                        ("updated", session.updated_at.as_str()),
                        ("logs", logs.as_str()),
                        ("commands", commands.as_str()),
                    ],
                )
            })
            .collect();
        if self.resume_store.sessions.len() > lines.len() {
            let count = (self.resume_store.sessions.len() - lines.len()).to_string();
            lines.push(trf("tui.resume.more", &[("count", count.as_str())]));
        }

        self.push_log(UiLevel::Info, tr("tui.resume.title"));
        for line in lines {
            self.push_log(UiLevel::Info, line);
        }
    }

    pub(super) fn switch_resume(&mut self, uid: &str) -> Result<(), String> {
        if uid == self.active_resume_uid {
            self.push_log(
                UiLevel::Info,
                trf("tui.resume.already_active", &[("uid", uid)]),
            );
            return Ok(());
        }

        self.sync_active_resume_snapshot();

        let session = self
            .resume_store
            .get(uid)
            .cloned()
            .ok_or_else(|| trf("tui.resume.not_found", &[("uid", uid)]))?;

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
            trf(
                "tui.resume.resumed",
                &[("uid", self.active_resume_uid.as_str())],
            ),
        );
        self.resume_dirty = true;
        Ok(())
    }

    pub(super) fn handle_console_command(&mut self, command: &str) -> CommandOutcome {
        let cmd = command.trim();
        let mut parts = cmd.split_whitespace();
        let command_name = parts.next().unwrap_or_default();
        let is_builtin = is_builtin_command_name(
            command_name,
            CommandScope::Tui,
            CommandNameOverrides::default(),
        );
        if is_builtin && !self.is_builtin_scope_command_enabled(CommandScope::Tui, command_name) {
            self.push_log(
                UiLevel::Warn,
                trf(
                    "tui.command.disabled_by_policy",
                    &[("command", command_name)],
                ),
            );
            return CommandOutcome::None;
        }
        match command_name {
            "/quit" | "/exit" => {
                self.push_log(UiLevel::Warn, tr("tui.command.shutdown_requested"));
                CommandOutcome::Quit
            }
            "/help" => {
                for line in render_builtin_help_lines_filtered(
                    CommandScope::Tui,
                    CommandNameOverrides::default(),
                    |_, name| self.is_builtin_scope_command_enabled(CommandScope::Tui, name),
                ) {
                    self.push_log(UiLevel::Info, line);
                }
                self.push_log(
                    UiLevel::Info,
                    trf(
                        "tui.command.active_resume",
                        &[("resume", self.active_resume_uid.as_str())],
                    ),
                );
                if let Some(plugin_sdk) = self.plugin_sdk.as_ref() {
                    let builtin_names =
                        builtin_command_names(CommandScope::Tui, CommandNameOverrides::default())
                            .into_iter()
                            .filter(|name| {
                                self.is_builtin_scope_command_enabled(CommandScope::Tui, name)
                            })
                            .collect::<Vec<_>>();
                    let plugin_commands = plugin_sdk
                        .list_scope_commands("tui")
                        .into_iter()
                        .filter(|entry| entry.enabled && entry.executable_in_tui)
                        .filter(|entry| {
                            builtin_names
                                .iter()
                                .all(|builtin_name| builtin_name != &entry.name)
                        })
                        .collect::<Vec<_>>();
                    if !plugin_commands.is_empty() {
                        self.push_log(
                            UiLevel::Info,
                            trf(
                                "tui.plugin_commands.title",
                                &[("count", plugin_commands.len().to_string().as_str())],
                            ),
                        );
                        for command in plugin_commands {
                            let description = tr(command.description.as_str());
                            self.push_log(
                                UiLevel::Info,
                                trf(
                                    "tui.plugin_commands.entry",
                                    &[
                                        ("name", command.name.as_str()),
                                        ("description", description.as_str()),
                                        ("plugin", command.plugin_id.as_str()),
                                    ],
                                ),
                            );
                        }
                    }
                }
                CommandOutcome::None
            }
            "/reload" => {
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, tr("tui.command.reload_usage"));
                    return CommandOutcome::None;
                }
                self.push_log(UiLevel::Info, tr("tui.command.reload_requested"));
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
                        tr("tui.command.log_view.entered")
                    } else {
                        tr("tui.command.log_view.returned")
                    };
                    self.push_log(UiLevel::Info, label);
                    return CommandOutcome::None;
                };
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, tr("tui.command.log_usage"));
                    return CommandOutcome::None;
                }
                match mode_arg {
                    "on" => {
                        self.set_view_mode(UiViewMode::LogConsole);
                        self.push_log(UiLevel::Info, tr("tui.command.log_view.entered"));
                    }
                    "off" => {
                        self.set_view_mode(UiViewMode::Dashboard);
                        self.push_log(UiLevel::Info, tr("tui.command.log_view.returned"));
                    }
                    _ => {
                        self.push_log(UiLevel::Warn, tr("tui.command.log_usage"));
                    }
                }
                CommandOutcome::None
            }
            "/clear" => {
                self.logs.clear();
                self.scroll_logs_bottom();
                self.push_log(UiLevel::Info, tr("tui.command.console_cleared"));
                CommandOutcome::None
            }
            "/adapters" => {
                if self.adapters.is_empty() {
                    self.push_log(UiLevel::Info, tr("tui.adapters.empty"));
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
                            tr("tui.adapter.status.run")
                        } else {
                            tr("tui.adapter.status.idle")
                        };
                        let transport = format!("{:?}", adapter.transport);
                        trf(
                            "tui.adapters.entry",
                            &[
                                ("adapter", adapter.id.as_str()),
                                ("status", status.as_str()),
                                ("transport", transport.as_str()),
                                ("url", adapter.endpoint.url.as_str()),
                            ],
                        )
                    })
                    .collect();
                for line in lines {
                    self.push_log(UiLevel::Info, line);
                }
                CommandOutcome::None
            }
            "/commands" => {
                let args: Vec<&str> = parts.collect();
                let action = match Self::parse_commands_action(&args) {
                    Ok(action) => action,
                    Err(_) => {
                        self.show_commands_usage();
                        return CommandOutcome::None;
                    }
                };
                match action {
                    CommandsAction::List(scope) => {
                        for line in self.command_catalog_lines(scope) {
                            self.push_log(UiLevel::Info, line);
                        }
                    }
                    CommandsAction::SetEnabled {
                        enabled,
                        scope,
                        name,
                    } => return self.set_scoped_command_enabled(scope, name, enabled),
                }
                CommandOutcome::None
            }
            "/plugins" => {
                let args: Vec<&str> = parts.collect();
                let action = match Self::parse_plugins_action(&args) {
                    Ok(action) => action,
                    Err(_) => {
                        self.show_plugins_usage();
                        return CommandOutcome::None;
                    }
                };
                match action {
                    PluginsAction::List => {
                        for line in self.plugin_catalog_lines() {
                            self.push_log(UiLevel::Info, line);
                        }
                    }
                    PluginsAction::SetEnabled { enabled, plugin_id } => {
                        return self.set_plugin_enabled(plugin_id, enabled);
                    }
                }
                CommandOutcome::None
            }
            "/ask" => {
                let prompt = parts.collect::<Vec<&str>>().join(" ").trim().to_string();
                if prompt.is_empty() {
                    self.push_log(UiLevel::Warn, tr("tui.command.ask_usage"));
                    return CommandOutcome::None;
                }
                self.push_log(UiLevel::Info, tr("tui.command.ask_started"));
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
                        trf(
                            "tui.command.resume_current",
                            &[("resume", self.active_resume_uid.as_str())],
                        ),
                    );
                    return CommandOutcome::None;
                };
                if parts.next().is_some() {
                    self.push_log(UiLevel::Warn, tr("tui.command.resume_usage"));
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
                if let Some(plugin_sdk) = self.plugin_sdk.as_ref()
                    && let Some(plugin_command) = plugin_sdk.get_tui_command(command_name)
                {
                    if !plugin_command.enabled {
                        self.push_log(
                            UiLevel::Warn,
                            trf(
                                "tui.command.plugin_disabled",
                                &[("command", plugin_command.name.as_str())],
                            ),
                        );
                        return CommandOutcome::None;
                    }
                    let args = parts.map(ToString::to_string).collect::<Vec<_>>();
                    return CommandOutcome::PluginCommand {
                        command: plugin_command.name,
                        args,
                    };
                }
                self.push_log(
                    UiLevel::Warn,
                    trf("tui.command.unknown", &[("command", cmd)]),
                );
                CommandOutcome::None
            }
        }
    }

    pub(super) fn command_help_text(&self) -> String {
        let input = self.console_input.trim();
        if input.is_empty() {
            if self.is_dashboard_plugin_panel_active() {
                if let Some(entry) = self.selected_dashboard_plugin_entry() {
                    let plugin_id = entry.descriptor.metadata.id;
                    let action = if self.is_plugin_enabled(plugin_id.as_str()) {
                        tr("tui.action.disable")
                    } else {
                        tr("tui.action.enable")
                    };
                    return tr("command.help.plugin.current")
                        .replace("{plugin}", plugin_id.as_str())
                        .replace("{action}", action.as_str());
                }
                return tr("command.help.empty.plugins.none").to_string();
            }
            if self.is_dashboard_view() {
                return tr("command.help.empty.command").to_string();
            }
            return tr("command.help.empty.log_view").to_string();
        }

        if !input.starts_with('/') {
            return tr("command.help.not_command").to_string();
        }

        if let Some(help) = self.command_help_for_line(input) {
            return help;
        }
        if let Some(help) = self.plugin_command_help_for_line(input) {
            return help;
        }

        if let Some((_, mode, candidates)) = self.completion_context()
            && let Some(candidate) = candidates.first()
        {
            let rendered = Self::apply_completion_candidate(mode, candidate);
            if let Some(help) = self.command_help_for_line(rendered.as_str()) {
                return help;
            }
            if let Some(help) = self.plugin_command_help_for_line(rendered.as_str()) {
                return help;
            }
        }

        tr("command.help.unknown").to_string()
    }

    pub(super) fn command_help_for_line(&self, line: &str) -> Option<String> {
        let mut parts = line.split_whitespace();
        let command = parts.next()?;
        if let Some(normalized) = normalize_builtin_command_name_for_scope(
            command,
            CommandScope::Tui,
            CommandNameOverrides::default(),
        ) && !self.is_builtin_scope_command_enabled(CommandScope::Tui, normalized.as_str())
        {
            return Some(format!(
                "{} {} {}",
                tr("command.help.prefix"),
                normalized,
                tr("command.help.disabled")
            ));
        }
        command_help_text_for_name(command, CommandScope::Tui, CommandNameOverrides::default())
    }

    fn plugin_command_help_for_line(&self, line: &str) -> Option<String> {
        let command = line.split_whitespace().next()?;
        let plugin_sdk = self.plugin_sdk.as_ref()?;
        let entry = plugin_sdk.get_tui_command(command)?;
        let detail = tr(entry.description.as_str());
        if entry.enabled {
            Some(
                tr("command.help.plugin.from.enabled")
                    .replace("{name}", entry.name.as_str())
                    .replace("{plugin}", entry.plugin_id.as_str())
                    .replace("{detail}", detail.as_str()),
            )
        } else {
            Some(
                tr("command.help.plugin.from.disabled")
                    .replace("{name}", entry.name.as_str())
                    .replace("{plugin}", entry.plugin_id.as_str()),
            )
        }
    }
}
