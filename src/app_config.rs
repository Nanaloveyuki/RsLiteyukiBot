use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use liteyukibot_core::AdapterConfig;
use serde::{Deserialize, Serialize};

use crate::i18n::{AppLocale, tr, trf, trf_for};
use crate::llm;
use crate::tui;
use crate::utils::config_path::{
    migrate_legacy_app_config_to_user_dir, replace_config_file, resolve_default_app_config_path,
    resolve_existing_app_config_path, resolve_existing_legacy_app_config_path,
    resolve_existing_user_app_config_path,
};

#[path = "app_config/access.rs"]
mod access;
#[path = "app_config/adapters.rs"]
mod adapters;
#[path = "app_config/resolve.rs"]
mod resolve;
#[path = "app_config/storage.rs"]
mod storage;
#[path = "app_config/validation.rs"]
mod validation;

pub(crate) use self::access::{
    resolve_disabled_plugins, resolve_disabled_scope_commands, resolve_help_whitelist,
    runtime_settings_values,
};
pub(crate) use self::adapters::load_adapter_configs;
pub(crate) use self::resolve::{
    resolve_app_locale, resolve_flow_local_agent_config, resolve_llm_config, resolve_tui_config,
};
pub(crate) use self::storage::{
    ensure_default_config_files, load_app_config_from_path, load_app_config_with_warnings,
    resolve_app_config_path,
};
pub(crate) use self::validation::{prime_reload_warning_state, validate_app_config};

static LAST_RELOAD_WARNING_STATE: LazyLock<Mutex<Option<ReloadWarningState>>> =
    LazyLock::new(|| Mutex::new(None));

fn localized_doc_text(doc: &AppConfigDoc, key: &str) -> String {
    trf_for(resolve_app_locale(doc), key, &[])
}

fn localized_doc_textf(doc: &AppConfigDoc, key: &str, args: &[(&str, &str)]) -> String {
    trf_for(resolve_app_locale(doc), key, args)
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AppConfigDoc {
    #[serde(default, rename = "core")]
    pub(crate) rust: Option<AppRustSection>,
    #[serde(default)]
    pub(crate) runtime: Option<RuntimeConfigSection>,
    #[serde(default)]
    pub(crate) log: Option<LogConfigSection>,
    #[serde(default)]
    pub(crate) adapters: Option<Vec<AdapterConfig>>,
    #[serde(default)]
    pub(crate) connect: Option<ConnectConfigSection>,
    #[serde(default)]
    pub(crate) tui: Option<TuiConfigSection>,
    #[serde(default)]
    pub(crate) i18n: Option<I18nConfigSection>,
    #[serde(default)]
    pub(crate) llm: Option<LlmConfigSection>,
    #[serde(default)]
    pub(crate) flow_local_agent: Option<FlowLocalAgentConfigSection>,
    #[serde(default)]
    pub(crate) commands: Option<CommandConfigSection>,
    #[serde(default)]
    pub(crate) plugins: Option<PluginConfigSection>,
    // 外部调用
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) desktop: Option<DesktopConfigSection>,
    #[serde(default, rename = "onebot-v11", alias = "onebot_v11")]
    pub(crate) onebot_v11: Option<OnebotV11ConfigSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct CommandConfigSection {
    #[serde(default)]
    pub(crate) disabled: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct PluginConfigSection {
    #[serde(default)]
    pub(crate) disabled: Vec<String>,
}

// 外部调用
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct DesktopConfigSection {
    #[serde(default)]
    pub(crate) close_to_tray: Option<bool>,
}

// 外部调用
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCloseBehavior {
    pub close_to_tray: bool,
    pub configured: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct OnebotV11ConfigSection {
    #[serde(default)]
    pub(crate) whitelist: Vec<OnebotWhitelistEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum OnebotWhitelistEntry {
    Text(String),
    Int(i64),
    UInt(u64),
}

impl OnebotWhitelistEntry {
    fn as_token(&self) -> String {
        match self {
            Self::Text(raw) => raw.trim().to_string(),
            Self::Int(raw) => raw.to_string(),
            Self::UInt(raw) => raw.to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AppRustSection {
    #[serde(default)]
    pub(crate) runtime: Option<RuntimeConfigSection>,
    #[serde(default)]
    pub(crate) log: Option<LogConfigSection>,
    #[serde(default)]
    pub(crate) adapters: Option<Vec<AdapterConfig>>,
    #[serde(default)]
    pub(crate) tui: Option<TuiConfigSection>,
    #[serde(default)]
    pub(crate) i18n: Option<I18nConfigSection>,
    #[serde(default)]
    pub(crate) commands: Option<CommandConfigSection>,
    #[serde(default)]
    pub(crate) plugins: Option<PluginConfigSection>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct RuntimeConfigSection {
    #[serde(default)]
    pub(crate) worker_count: Option<usize>,
    #[serde(default)]
    pub(crate) ingress_queue: Option<usize>,
    #[serde(default)]
    pub(crate) worker_queue: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct LogConfigSection {
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) level: Option<String>,
    #[serde(default)]
    pub(crate) timezone: Option<String>,
    #[serde(default)]
    pub(crate) timestamp_format: Option<String>,
    #[serde(default)]
    pub(crate) timestamp_pattern: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct ConnectConfigSection {
    #[serde(default)]
    pub(crate) websocket: Option<WebSocketConnectSection>,
    #[serde(default, rename = "tcp-http")]
    pub(crate) tcp_http: Option<HttpConnectSection>,
    #[serde(default)]
    pub(crate) sse: Option<SseConnectSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct WebSocketConnectSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default, alias = "endpoints", alias = "multi_urls")]
    pub(crate) urls: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) queue_capacity: Option<usize>,
    #[serde(default)]
    pub(crate) max_payload_size: Option<usize>,
    #[serde(default)]
    pub(crate) max_connections: Option<usize>,
    #[serde(default)]
    pub(crate) inbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) outbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) forward: Option<WebSocketEndpointSection>,
    #[serde(default)]
    pub(crate) reverse: Option<WebSocketEndpointSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct WebSocketEndpointSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default, alias = "endpoints", alias = "multi_urls")]
    pub(crate) urls: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) queue_capacity: Option<usize>,
    #[serde(default)]
    pub(crate) max_payload_size: Option<usize>,
    #[serde(default)]
    pub(crate) max_connections: Option<usize>,
    #[serde(default)]
    pub(crate) inbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct HttpConnectSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default, alias = "endpoints", alias = "multi_urls")]
    pub(crate) urls: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) queue_capacity: Option<usize>,
    #[serde(default)]
    pub(crate) max_payload_size: Option<usize>,
    #[serde(default)]
    pub(crate) max_connections: Option<usize>,
    #[serde(default)]
    pub(crate) inbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SseConnectSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default, alias = "endpoints", alias = "multi_urls")]
    pub(crate) urls: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) queue_capacity: Option<usize>,
    #[serde(default)]
    pub(crate) max_payload_size: Option<usize>,
    #[serde(default)]
    pub(crate) max_connections: Option<usize>,
    #[serde(default)]
    pub(crate) inbound_topic: Option<String>,
    #[serde(default)]
    pub(crate) outbound_topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct TuiConfigSection {
    #[serde(default)]
    pub(crate) resume: Option<TuiResumeSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct TuiResumeSection {
    #[serde(default)]
    pub(crate) store_path: Option<String>,
    #[serde(default)]
    pub(crate) max_sessions: Option<usize>,
    #[serde(default)]
    pub(crate) max_size_mib: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct I18nConfigSection {
    #[serde(default)]
    pub(crate) locale: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct LlmConfigSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) stream: Option<bool>,
    #[serde(default)]
    pub(crate) provider: Option<String>,
    #[serde(default)]
    pub(crate) base_url: Option<String>,
    #[serde(default)]
    pub(crate) provider_urls: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) api_keys: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) api_key: Option<String>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) temperature: Option<f32>,
    #[serde(default)]
    pub(crate) top_p: Option<f32>,
    #[serde(default)]
    pub(crate) top_k: Option<u32>,
    #[serde(default)]
    pub(crate) frequency_penalty: Option<f32>,
    #[serde(default)]
    pub(crate) presence_penalty: Option<f32>,
    #[serde(default)]
    pub(crate) parallel_tool_calls: Option<bool>,
    #[serde(default)]
    pub(crate) system_prompt: Option<String>,
    #[serde(default)]
    pub(crate) command_prefix: Option<String>,
    #[serde(default)]
    pub(crate) active_provider_id: Option<String>,
    #[serde(default)]
    pub(crate) providers: Option<Vec<LlmManagedProviderConfig>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct FlowLocalAgentConfigSection {
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) base_url: Option<String>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) device_id: Option<String>,
    #[serde(default)]
    pub(crate) device_name: Option<String>,
    #[serde(default)]
    pub(crate) auto_connect: Option<bool>,
    #[serde(default)]
    pub(crate) allowed_tools: Vec<String>,
    #[serde(default)]
    pub(crate) workspace_root: Option<String>,
    #[serde(default)]
    pub(crate) command_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) approval_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(crate) struct LlmManagedProviderConfig {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) provider: Option<String>,
    #[serde(default)]
    pub(crate) base_url: Option<String>,
    #[serde(default)]
    pub(crate) api_key: Option<String>,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) models: Option<Vec<LlmManagedModelConfig>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(crate) struct LlmManagedModelConfig {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
}

#[derive(Debug, Clone)]
pub(crate) struct LlmRuntimeConfig {
    pub(crate) enabled: bool,
    pub(crate) stream: bool,
    pub(crate) provider: String,
    pub(crate) base_url: String,
    pub(crate) api_keys: Vec<String>,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) model: String,
    pub(crate) timeout_ms: u64,
    pub(crate) temperature: Option<f32>,
    pub(crate) top_p: Option<f32>,
    pub(crate) top_k: Option<u32>,
    pub(crate) frequency_penalty: Option<f32>,
    pub(crate) presence_penalty: Option<f32>,
    pub(crate) parallel_tool_calls: bool,
    pub(crate) system_prompt: Option<String>,
    pub(crate) command_prefix: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct FlowLocalAgentRuntimeConfig {
    pub(crate) enabled: bool,
    pub(crate) base_url: Option<String>,
    pub(crate) token: Option<String>,
    pub(crate) device_id: Option<String>,
    pub(crate) device_name: Option<String>,
    pub(crate) auto_connect: bool,
    pub(crate) allowed_tools: Vec<String>,
    pub(crate) workspace_root: Option<PathBuf>,
    pub(crate) command_timeout_ms: u64,
    pub(crate) approval_policy: String,
}

impl llm::OpenAiRuntimeConfig for LlmRuntimeConfig {
    fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    fn model(&self) -> &str {
        self.model.as_str()
    }

    fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    fn stream(&self) -> bool {
        self.stream
    }

    fn temperature(&self) -> Option<f32> {
        self.temperature
    }

    fn top_p(&self) -> Option<f32> {
        self.top_p
    }

    fn top_k(&self) -> Option<u32> {
        self.top_k
    }

    fn frequency_penalty(&self) -> Option<f32> {
        self.frequency_penalty
    }

    fn presence_penalty(&self) -> Option<f32> {
        self.presence_penalty
    }

    fn parallel_tool_calls(&self) -> bool {
        self.parallel_tool_calls
    }

    fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    fn default_headers(&self) -> Option<&HashMap<String, String>> {
        Some(&self.headers)
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdapterConfigDoc {
    pub(crate) adapters: Vec<AdapterConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ReloadWarningState {
    runtime: Option<RuntimeConfigSection>,
    log: Option<LogConfigSection>,
}

impl ReloadWarningState {
    pub(crate) fn from_doc(doc: &AppConfigDoc) -> Self {
        Self {
            runtime: access::config_runtime(doc).cloned(),
            log: access::config_log(doc).cloned(),
        }
    }
}

pub(crate) fn write_default_config_if_missing(
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    storage::write_default_config_if_missing(path)
}

pub(crate) fn collect_runtime_reload_warnings(doc: &AppConfigDoc) -> Vec<String> {
    validation::collect_runtime_reload_warnings(doc)
}

// 外部调用
#[allow(dead_code)]
pub(crate) fn runtime_reload_warnings(
    previous: Option<&ReloadWarningState>,
    current: &ReloadWarningState,
) -> Vec<String> {
    validation::runtime_reload_warnings(previous, current)
}

// 外部调用
#[allow(dead_code)]
pub fn resolve_desktop_close_behavior() -> DesktopCloseBehavior {
    storage::resolve_desktop_close_behavior()
}

// 外部调用
#[allow(dead_code)]
pub fn persist_desktop_close_to_tray_preference(
    close_to_tray: bool,
) -> Result<DesktopCloseBehavior, String> {
    storage::persist_desktop_close_to_tray_preference(close_to_tray)
}

const DEFAULT_YAML_CONFIG_TEMPLATE: &str = r#"core:
  runtime:
    # also used by adapter parallel pools (WS forward / SSE / HTTP)
    worker_count: 4
    ingress_queue: 1024
    worker_queue: 256
  log:
    mode: color
    level: info
    timezone: local
    timestamp_format: custom
    timestamp_pattern: "%Y-%m-%d %H:%M:%S"
  adapters: []
  tui:
    resume:
      store_path: ./.liteyuki-tui-resumes.json
      max_sessions: 64
      max_size_mib: 16

i18n:
  # "zh-CN" | "en-US"
  locale: "zh-CN"

connect:
  websocket:
    enabled: true
    # mode: forward | reverse | both
    mode: reverse
    # 多端同连: urls 可配置多个地址，自动展开为 connect-ws-*-1/-2...
    # urls:
    #   - ws://127.0.0.1:3000/ws
    #   - ws://127.0.0.1:3001/ws
    # reverse mode can use port (+ optional host/path)
    host: 0.0.0.0
    port: 8080
    path: /ws
    # forward mode can use url directly
    # url: ws://127.0.0.1:3000/ws
    max_payload_size: 1048576
    max_connections: 100
    timeout_seconds: 30
  tcp-http:
    enabled: true
    # 多端同连:
    # urls:
    #   - http://127.0.0.1:8081/
    #   - http://127.0.0.1:8083/
    host: 127.0.0.1
    port: 8081
    path: /
    max_payload_size: 1048576
    max_connections: 100
    timeout_seconds: 30
  sse:
    enabled: true
    # 多端同连:
    # urls:
    #   - http://127.0.0.1:8082/sse
    #   - http://127.0.0.1:8084/sse
    host: 127.0.0.1
    port: 8082
    path: /sse
    max_payload_size: 1048576
    max_connections: 100
    timeout_seconds: 30

llm:
  enabled: false
  # stream: true
  provider: openai
  model: gpt-4.1-mini
  timeout_seconds: 20
  # temperature: 0.7
  # top_p: 1.0
  # top_k: 40 # compatibility providers only; ignored for official OpenAI Responses
  # frequency_penalty: 0.0
  # presence_penalty: 0.0
  # parallel_tool_calls: true
  command_prefix: /ask
  # provider_urls:
  #   - https://api.openai.com
  # api_keys:
  #   - sk-xxx
  # api_key: sk-xxx
  # system_prompt: "You are a helpful assistant."

flow_local_agent:
  enabled: false
  auto_connect: true
  command_timeout_seconds: 30
  approval_policy: prompt
  # base_url: https://flow.liteyuki.org
  # token: lys_xxx
  # device_id: 00000000-0000-0000-0000-000000000000
  # device_name: My Server
  # workspace_root: ./
  # allowed_tools:
  #   - run_command
  #   - read_file
  #   - write_file
  #   - list_files

commands:
  # 格式: "<scope> <name>"
  # - "tui /help"
  # - "adapter:onebot11 /su"
  disabled: []

plugins:
  # 按 plugin id 禁用插件
  # - "builtin-liteecho"
  disabled: []

onebot-v11:
  # 仅白名单会话可触发外部 /help。
  # 可写纯ID（private常用 user_id；group常用 group_id）或带前缀:
  # - "3541766758"
  # - "private:3541766758"
  # - "group:699493240"
  whitelist: []
"#;

const DEFAULT_TOML_CONFIG_TEMPLATE: &str = r#"[core]
adapters = []

[core.runtime]
# also used by adapter parallel pools (WS forward / SSE / HTTP)
worker_count = 4
ingress_queue = 1024
worker_queue = 256

[core.log]
mode = "color"
level = "info"
timezone = "local"
timestamp_format = "custom"
timestamp_pattern = "%Y-%m-%d %H:%M:%S"

[core.tui.resume]
store_path = "./.liteyuki-tui-resumes.json"
max_sessions = 64
max_size_mib = 16

[i18n]
# locale = "zh-CN" # or "en-US"
locale = "zh-CN"

[connect.websocket]
enabled = true
mode = "reverse" # forward | reverse | both
# multi-end:
# urls = ["ws://127.0.0.1:3000/ws", "ws://127.0.0.1:3001/ws"]
host = "0.0.0.0"
port = 8080
path = "/ws"
max_payload_size = 1048576
max_connections = 100
timeout_seconds = 30

[connect.tcp-http]
enabled = true
# multi-end:
# urls = ["http://127.0.0.1:8081/", "http://127.0.0.1:8083/"]
host = "127.0.0.1"
port = 8081
path = "/"
max_payload_size = 1048576
max_connections = 100
timeout_seconds = 30

[connect.sse]
enabled = true
# multi-end:
# urls = ["http://127.0.0.1:8082/sse", "http://127.0.0.1:8084/sse"]
host = "127.0.0.1"
port = 8082
path = "/sse"
max_payload_size = 1048576
max_connections = 100
timeout_seconds = 30

[llm]
enabled = false
# stream = true
provider = "openai"
model = "gpt-4.1-mini"
timeout_seconds = 20
# temperature = 0.7
# top_p = 1.0
# top_k = 40 # compatibility providers only; ignored for official OpenAI Responses
# frequency_penalty = 0.0
# presence_penalty = 0.0
# parallel_tool_calls = true
command_prefix = "/ask"
# provider_urls = ["https://api.openai.com"]
# api_keys = ["sk-xxx"]
# api_key = "sk-xxx"
# system_prompt = "You are a helpful assistant."

[flow_local_agent]
enabled = false
auto_connect = true
command_timeout_seconds = 30
approval_policy = "prompt"
# base_url = "https://flow.liteyuki.org"
# token = "lys_xxx"
# device_id = "00000000-0000-0000-0000-000000000000"
# device_name = "My Server"
# workspace_root = "./"
# allowed_tools = ["run_command", "read_file", "write_file", "list_files"]

[commands]
# disabled = ["tui /help", "adapter:onebot11 /su"]
disabled = []

[plugins]
# disabled = ["builtin-liteecho"]
disabled = []

[onebot-v11]
# whitelist = ["3541766758", "private:3541766758", "group:699493240"]
whitelist = []
"#;

fn normalize_non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
}

fn normalize_non_empty_list(values: Option<&Vec<String>>) -> Vec<String> {
    let Some(values) = values else {
        return Vec::new();
    };

    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if seen.insert(value.to_string()) {
            output.push(value.to_string());
        }
    }
    output
}

fn indexed_adapter_id(base_id: &str, index: usize, total: usize) -> String {
    if total <= 1 {
        base_id.to_string()
    } else {
        format!("{base_id}-{}", index + 1)
    }
}

fn has_non_empty_list(values: Option<&Vec<String>>) -> bool {
    !normalize_non_empty_list(values).is_empty()
}

fn has_empty_item(values: Option<&Vec<String>>) -> bool {
    values.is_some_and(|items| items.iter().any(|item| item.trim().is_empty()))
}

fn websocket_has_multi_urls(
    endpoint: &WebSocketEndpointSection,
    fallback: &WebSocketConnectSection,
) -> bool {
    has_non_empty_list(endpoint.urls.as_ref()) || has_non_empty_list(fallback.urls.as_ref())
}

fn push_should_be_positive_warning(warnings: &mut Vec<String>, locale: AppLocale, field: &str) {
    warnings.push(trf_for(
        locale,
        "config.warn.should_be_positive",
        &[("field", field)],
    ));
}

fn push_should_not_be_empty_warning(warnings: &mut Vec<String>, locale: AppLocale, field: &str) {
    warnings.push(trf_for(
        locale,
        "config.warn.should_not_be_empty",
        &[("field", field)],
    ));
}

fn push_empty_values_warning(warnings: &mut Vec<String>, locale: AppLocale, field: &str) {
    warnings.push(trf_for(
        locale,
        "config.warn.empty_values",
        &[("field", field)],
    ));
}

fn push_invalid_range_warning(
    warnings: &mut Vec<String>,
    locale: AppLocale,
    field: &str,
    range: &str,
) {
    warnings.push(trf_for(
        locale,
        "config.warn.invalid_range",
        &[("field", field), ("range", range)],
    ));
}

pub(crate) fn build_url(scheme: &str, host: &str, port: Option<u16>, path: &str) -> Option<String> {
    let port = port?;
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Some(format!("{scheme}://{host}:{port}{normalized_path}"))
}

pub(crate) fn seconds_to_timeout_ms(seconds: Option<u64>) -> u64 {
    seconds.unwrap_or(5).saturating_mul(1000).max(10)
}

pub(crate) fn sanitize_adapter_configs(
    configs: Vec<AdapterConfig>,
    source: &str,
) -> Vec<AdapterConfig> {
    let mut sanitized = Vec::new();
    let mut seen = HashSet::new();
    for config in configs {
        if let Err(err) = config.validate() {
            eprintln!(
                "{}",
                trf(
                    "config.stderr.skip_invalid_adapter",
                    &[
                        ("source", source),
                        ("adapter", config.id.as_str()),
                        ("err", err.as_str()),
                    ],
                )
            );
            continue;
        }
        if !seen.insert(config.id.clone()) {
            eprintln!(
                "{}",
                trf(
                    "config.stderr.skip_duplicate_adapter",
                    &[("source", source), ("adapter", config.id.as_str())],
                )
            );
            continue;
        }
        sanitized.push(config);
    }
    sanitized
}

fn parse_bool_env(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}
