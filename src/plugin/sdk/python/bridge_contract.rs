pub(super) const PYTHON_ROOT_MODULE: &str = "liteyuki";
pub(super) const PYTHON_SDK_MODULE: &str = "liteyuki_sdk";
pub(super) const PYTHON_BRIDGE_SDK_GLOBAL: &str = "__bridge_sdk__";
pub(super) const PYTHON_BOOTSTRAP_HANDLER_ATTR: &str = "on_load";
pub(super) const PYTHON_RUNTIME_OPTION_EVENT_HANDLER: &str = "event_handler";
pub(super) const PYTHON_RUNTIME_OPTION_START_HANDLER: &str = "start_handler";
pub(super) const PYTHON_RUNTIME_OPTION_HEALTH_HANDLER: &str = "health_handler";
pub(super) const PYTHON_RUNTIME_OPTION_SHUTDOWN_HANDLER: &str = "shutdown_handler";
pub(super) const PYTHON_RUNTIME_OPTION_UNLOAD_HANDLER: &str = "unload_handler";
pub(super) const PYTHON_RUNTIME_OPTION_CONFIG_PATH: &str = "config_path";

pub(super) const ASTRBOT_BIND_RUNTIME_FN: &str = "_bind_astrbot_plugin_runtime";
pub(super) const ASTRBOT_CLEANUP_RUNTIME_FN: &str = "_cleanup_astrbot_plugin_runtime";
pub(super) const ASTRBOT_GET_RUNTIME_FN: &str = "_get_astrbot_plugin_runtime";
pub(super) const ASTRBOT_SNAPSHOT_RUNTIME_FN: &str = "_snapshot_astrbot_plugin_runtime";
pub(super) const ASTRBOT_INVOKE_WEB_HANDLER_FN: &str = "_invoke_astrbot_web_handler";
pub(super) const ASTRBOT_REQUIRED_RUNTIME_ATTRS: [&str; 5] = [
    ASTRBOT_BIND_RUNTIME_FN,
    ASTRBOT_CLEANUP_RUNTIME_FN,
    ASTRBOT_GET_RUNTIME_FN,
    ASTRBOT_INVOKE_WEB_HANDLER_FN,
    ASTRBOT_SNAPSHOT_RUNTIME_FN,
];

pub(super) const ASTRBOT_RUNTIME_KEY_TOOLS: &str = "llm_tools";
pub(super) const ASTRBOT_RUNTIME_KEY_CRON_JOBS: &str = "cron_jobs";
pub(super) const ASTRBOT_RUNTIME_KEY_REGISTERED_WEB_APIS: &str = "registered_web_apis";

pub(super) const PYTHON_WEB_API_ROUTE_INDEX: usize = 0;
pub(super) const PYTHON_WEB_API_HANDLER_INDEX: usize = 1;
pub(super) const PYTHON_WEB_API_METHODS_INDEX: usize = 2;

pub(super) const PYTHON_TOOL_ATTR_NAME: &str = "name";
pub(super) const PYTHON_TOOL_ATTR_ACTIVE: &str = "active";
pub(super) const PYTHON_TOOL_CALL_METHOD: &str = "call";

pub(super) const PYTHON_CRON_ATTR_JOB_ID: &str = "job_id";
pub(super) const PYTHON_CRON_ATTR_ENABLED: &str = "enabled";
pub(super) const PYTHON_CRON_ATTR_HANDLER: &str = "handler";

pub(super) const PYTHON_EVENT_HANDLER_ATTRS: [&str; 3] =
    ["on_event", "handle_event", "liteyuki_handle_event"];
pub(super) const PYTHON_START_HANDLER_ATTRS: [&str; 3] = ["on_start", "start", "liteyuki_start"];
pub(super) const PYTHON_HEALTH_HANDLER_ATTRS: [&str; 3] =
    ["on_health_check", "health_check", "liteyuki_health_check"];
pub(super) const PYTHON_UNLOAD_HANDLER_ATTRS: [&str; 3] =
    ["on_unload", "unload", "liteyuki_unload"];
pub(super) const PYTHON_SHUTDOWN_HANDLER_ATTRS: [&str; 3] =
    ["on_shutdown", "shutdown", "liteyuki_shutdown"];
