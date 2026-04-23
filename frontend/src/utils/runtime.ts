const HTTP_PROTOCOLS = new Set(['http:', 'https:']);
const WS_PROTOCOL_BY_HTTP: Record<string, string> = {
  'http:': 'ws:',
  'https:': 'wss:',
};

declare global {
  interface Window {
    __LITEYUKI_RUNTIME_API_BASE__?: string;
    __LITEYUKI_LOCAL_TOKEN__?: string;
  }
}

function normalizeRuntimePath (path: string): string {
  if (!path) {
    return '/';
  }

  if (/^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(path) || path.startsWith('//')) {
    return path;
  }

  return path.startsWith('/') ? path : `/${path}`;
}

function normalizeHttpBase (raw?: string): string | null {
  if (!raw) {
    return null;
  }

  try {
    const url = new URL(raw);
    if (!HTTP_PROTOCOLS.has(url.protocol)) {
      return null;
    }

    url.search = '';
    url.hash = '';
    const pathname = url.pathname.replace(/\/+$/, '');
    return pathname && pathname !== '/'
      ? `${url.origin}${pathname}`
      : url.origin;
  } catch {
    return null;
  }
}

function browserHttpBase (): string | null {
  if (typeof window === 'undefined') {
    return null;
  }

  return HTTP_PROTOCOLS.has(window.location.protocol)
    ? normalizeHttpBase(window.location.origin)
    : null;
}

export function resolveRuntimeApiBase (): string | null {
  return normalizeHttpBase(import.meta.env.VITE_API_BASE)
    ?? normalizeHttpBase(window.__LITEYUKI_RUNTIME_API_BASE__)
    ?? browserHttpBase();
}

export function resolveRuntimeHttpUrl (path: string): string {
  const normalizedPath = normalizeRuntimePath(path);
  if (/^https?:\/\//i.test(normalizedPath)) {
    return normalizedPath;
  }

  const base = resolveRuntimeApiBase();
  if (!base) {
    throw new Error('Runtime API base is unavailable in the current environment');
  }

  return new URL(normalizedPath, `${base}/`).toString();
}

export function resolveApiUrl (path: string): string {
  const normalizedPath = normalizeRuntimePath(path);
  const apiPath = normalizedPath === '/api' || normalizedPath.startsWith('/api/')
    ? normalizedPath
    : `/api${normalizedPath}`;
  return resolveRuntimeHttpUrl(apiPath);
}

export function resolveRuntimeWebSocketUrl (path: string): string {
  const normalizedPath = normalizeRuntimePath(path);
  if (/^wss?:\/\//i.test(normalizedPath)) {
    return normalizedPath;
  }

  const url = new URL(resolveRuntimeHttpUrl(normalizedPath));
  const wsProtocol = WS_PROTOCOL_BY_HTTP[url.protocol];
  if (!wsProtocol) {
    throw new Error(`Unsupported runtime protocol for websocket: ${url.protocol}`);
  }

  url.protocol = wsProtocol;
  return url.toString();
}

export function resolveRuntimeAssetUrl (path: string): string {
  const normalizedPath = normalizeRuntimePath(path);
  if (/^https?:\/\//i.test(normalizedPath)) {
    return normalizedPath;
  }

  return resolveRuntimeHttpUrl(normalizedPath);
}
