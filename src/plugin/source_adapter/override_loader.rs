#[path = "override_loader/discovery.rs"]
mod discovery;
#[path = "override_loader/paths.rs"]
mod paths;
#[path = "override_loader/synthesis.rs"]
mod synthesis;

pub use discovery::discover_plugin_manifests_in_dirs;
