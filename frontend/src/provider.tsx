import type { PropsWithChildren } from "react";

import { HeroUIProvider } from "@heroui/system";
import { useNavigate } from "react-router-dom";

import { RuntimeHealthProvider } from "@/contexts/runtime-health";
import { ThemeModeProvider } from "@/contexts/theme-mode";

export function Provider({ children }: PropsWithChildren) {
  const navigate = useNavigate();

  return (
    <HeroUIProvider navigate={navigate}>
      <ThemeModeProvider>
        <RuntimeHealthProvider>{children}</RuntimeHealthProvider>
      </ThemeModeProvider>
    </HeroUIProvider>
  );
}
