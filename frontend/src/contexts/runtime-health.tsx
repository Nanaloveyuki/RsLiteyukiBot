import { createContext, useContext, type PropsWithChildren } from "react";

import { useRuntimeHealth } from "@/hooks/useRuntimeHealth";

const RuntimeHealthContext = createContext<ReturnType<typeof useRuntimeHealth> | null>(null);

export function RuntimeHealthProvider({ children }: PropsWithChildren) {
  const runtimeHealth = useRuntimeHealth();

  return <RuntimeHealthContext.Provider value={runtimeHealth}>{children}</RuntimeHealthContext.Provider>;
}

export function useRuntimeHealthContext() {
  const context = useContext(RuntimeHealthContext);

  if (!context) {
    throw new Error("useRuntimeHealthContext must be used within RuntimeHealthProvider");
  }

  return context;
}
