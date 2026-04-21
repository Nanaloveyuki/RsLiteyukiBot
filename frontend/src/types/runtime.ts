export interface ExternalStats {
  command_hits: number;
  api_requests: number;
  api_success: number;
  api_failed: number;
  api_timeouts: number;
  api_inflight: number;
}

export interface ResourceCpuUsage {
  system_percent: number;
  process_percent: number;
}

export interface ResourceMemoryUsage {
  total_bytes: number;
  used_bytes: number;
  process_bytes: number;
  system_percent: number;
  process_percent: number;
}

export interface ResourceUsage {
  cpu: ResourceCpuUsage;
  memory: ResourceMemoryUsage;
}

export interface RuntimeSnapshot {
  app_name: string;
  status: string;
  runtime_target: string;
  locale: string;
  runtime_config: string;
  adapter_count: number;
  adapter_autostart: boolean;
  plugin_dirs: string[];
  disabled_commands: string[];
  disabled_plugins: string[];
  llm_command_prefix: string;
  help_whitelist_size: number;
  warnings: string[];
  notes: string[];
  last_event_topic: string | null;
  last_event_preview: string | null;
  handled_events: number;
  external_stats: ExternalStats;
  resource_usage: ResourceUsage;
}

export interface HealthPayload {
  bind: string;
  desktop_url: string;
  external_url_hint: string;
  runtime: RuntimeSnapshot;
}

export interface RuntimeLogEntry {
  timestamp: string;
  level: string;
  module: string;
  message: string;
  line: string;
}

export interface RuntimeLogsPayload {
  entries: RuntimeLogEntry[];
}

export type RuntimeTone = "good" | "warm" | "cold";
