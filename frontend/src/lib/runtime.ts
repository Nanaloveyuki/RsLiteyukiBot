const DEFAULT_RUNTIME_API_BASE = "http://127.0.0.1:14500";

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}

export function resolveApiBase(): string {
  const configuredBase = import.meta.env.VITE_RUNTIME_API_BASE?.trim();

  if (configuredBase) {
    return trimTrailingSlash(configuredBase);
  }

  const isBrowserProtocol =
    window.location.protocol === "http:" || window.location.protocol === "https:";

  if (!isBrowserProtocol) {
    return DEFAULT_RUNTIME_API_BASE;
  }

  if (import.meta.env.DEV && window.location.port === "1420") {
    return DEFAULT_RUNTIME_API_BASE;
  }

  return trimTrailingSlash(window.location.origin);
}

export function resolveRuntimeUrl(path: string): string {
  return `${resolveApiBase()}${path.startsWith("/") ? path : `/${path}`}`;
}

export function resolveHealthUrl(): string {
  return resolveRuntimeUrl("/api/health");
}

export function resolveLogsUrl(): string {
  return resolveRuntimeUrl("/api/logs");
}
