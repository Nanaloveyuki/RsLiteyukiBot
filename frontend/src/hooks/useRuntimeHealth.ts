import { useCallback, useEffect, useMemo, useState } from "react";

import { resolveHealthUrl } from "@/lib/runtime";
import type { HealthPayload, RuntimeTone } from "@/types/runtime";

export function useRuntimeHealth(pollIntervalMs = 5000) {
  const [health, setHealth] = useState<HealthPayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [updatedAt, setUpdatedAt] = useState("");

  const refresh = useCallback(async () => {
    try {
      const response = await fetch(resolveHealthUrl(), {
        cache: "no-store",
      });

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }

      setHealth((await response.json()) as HealthPayload);
      setError("");
      setUpdatedAt(new Date().toLocaleString());
    } catch (fetchError) {
      setError(`failed to load runtime health: ${String(fetchError)}`);
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

  const statusTone = useMemo<RuntimeTone>(() => {
    const status = health?.runtime.status ?? "";
    if (status === "running") {
      return "good";
    }
    if (status === "starting") {
      return "warm";
    }
    return "cold";
  }, [health]);

  return {
    health,
    loading,
    error,
    updatedAt,
    refresh,
    statusTone,
  };
}
