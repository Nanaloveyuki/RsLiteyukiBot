import { useCallback, useEffect, useState } from "react";

import { resolveLogsUrl } from "@/lib/runtime";
import type { RuntimeLogEntry, RuntimeLogsPayload } from "@/types/runtime";

export function useRuntimeLogs(pollIntervalMs = 1500) {
  const [entries, setEntries] = useState<RuntimeLogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [updatedAt, setUpdatedAt] = useState("");

  const refresh = useCallback(async () => {
    try {
      const response = await fetch(resolveLogsUrl(), {
        cache: "no-store",
      });

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }

      const payload = (await response.json()) as RuntimeLogsPayload;
      setEntries(payload.entries ?? []);
      setError("");
      setUpdatedAt(new Date().toLocaleString());
    } catch (fetchError) {
      setError(`failed to load runtime logs: ${String(fetchError)}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => {
      void refresh();
    }, pollIntervalMs);

    return () => {
      window.clearInterval(timer);
    };
  }, [pollIntervalMs, refresh]);

  return {
    entries,
    loading,
    error,
    updatedAt,
    refresh,
  };
}
