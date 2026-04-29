import key from '@/const/key';

export function normalizeStoredStringValue (raw: string | null | undefined): string | null {
  const candidate = raw?.trim();
  if (!candidate) {
    return null;
  }

  try {
    const parsed = JSON.parse(candidate);
    return typeof parsed === 'string' ? parsed : candidate;
  } catch {
    return candidate;
  }
}

export function readStoredString (storageKey: string): string | null {
  if (typeof window === 'undefined') {
    return null;
  }

  const raw = window.localStorage.getItem(storageKey);
  return normalizeStoredStringValue(raw);
}

export function readStoredAuthToken (): string | null {
  const token = readStoredString(key.token)?.trim();
  return token ? token : null;
}

export function buildBearerAuthHeader (token: string | null = readStoredAuthToken()): Record<string, string> {
  const normalizedToken = normalizeStoredStringValue(token);
  if (!normalizedToken) {
    return {};
  }

  return {
    Authorization: `Bearer ${normalizedToken}`,
  };
}
