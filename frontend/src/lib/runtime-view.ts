import type { HealthPayload, ResourceUsage, RuntimeSnapshot } from "@/types/runtime";

export function getWarnings(runtime: RuntimeSnapshot | null | undefined): string[] {
  return runtime?.warnings ?? [];
}

export function getNotes(runtime: RuntimeSnapshot | null | undefined): string[] {
  return runtime?.notes ?? [];
}

export function getPluginDirs(runtime: RuntimeSnapshot | null | undefined): string[] {
  return runtime?.plugin_dirs ?? [];
}

export function getDisabledCommands(runtime: RuntimeSnapshot | null | undefined): string[] {
  return runtime?.disabled_commands ?? [];
}

export function getDisabledPlugins(runtime: RuntimeSnapshot | null | undefined): string[] {
  return runtime?.disabled_plugins ?? [];
}

export function getBind(health: HealthPayload | null | undefined): string {
  return health?.bind ?? "0.0.0.0:14500";
}

export function getDesktopUrl(health: HealthPayload | null | undefined): string {
  return health?.desktop_url ?? "http://127.0.0.1:14500/";
}

export function getExternalUrlHint(health: HealthPayload | null | undefined): string {
  return health?.external_url_hint ?? "http://<host-ip>:14500/";
}

export function getSuccessRate(runtime: RuntimeSnapshot | null | undefined): number {
  if (!runtime?.external_stats.api_requests) {
    return 0;
  }

  return Math.round((runtime.external_stats.api_success / runtime.external_stats.api_requests) * 100);
}

export function getResourceUsage(runtime: RuntimeSnapshot | null | undefined): ResourceUsage | null {
  return runtime?.resource_usage ?? null;
}

export function hasResourceUsage(resourceUsage: ResourceUsage | null | undefined): boolean {
  return (resourceUsage?.memory.total_bytes ?? 0) > 0;
}

export function formatPercent(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "--";
  }

  return `${Math.round(value)}%`;
}

export function formatBytes(bytes: number | null | undefined): string {
  if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const precision = value >= 100 || unitIndex === 0 ? 0 : 1;

  return `${value.toFixed(precision)} ${units[unitIndex]}`;
}
