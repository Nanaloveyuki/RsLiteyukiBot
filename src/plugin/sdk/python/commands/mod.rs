mod declared;
mod models;
mod normalization;
mod scope_state;
mod tui_registry;

pub use models::{PluginScopedCommand, PluginTuiCommand};

pub(crate) use declared::{disabled_declared_command_for_plugin, register_declared_commands};
pub(crate) use normalization::normalize_tui_command_name;
pub(crate) use scope_state::{
    is_builtin_command_disabled_in_lock, is_scope_command_disabled, list_disabled_scope_commands,
    list_scope_commands, set_builtin_command_enabled_in_lock, set_scope_command_enabled,
    sync_disabled_scope_commands_in_lock,
};
pub(crate) use tui_registry::{
    list_tui_commands, register_tui_command, remove_tui_command, set_tui_command_enabled,
};
