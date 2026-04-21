import { Button } from "@heroui/button";
import clsx from "clsx";

import type { MenuItem } from "@/config/site";

interface MenusProps {
  items: MenuItem[];
  activePath: string;
  onNavigate: (href: string) => void;
}

export default function Menus({ items, activePath, onNavigate }: MenusProps) {
  return (
    <div className="flex flex-1 flex-col justify-center gap-2">
      {items.map((item) => {
        const isActive = item.href === activePath;

        return (
          <div key={item.id}>
            <Button
              className={clsx(
                "group flex w-full items-center justify-start rounded-2xl px-3 py-2.5 text-left transition-all duration-300 dark:text-white",
                isActive
                  ? "translate-x-1 bg-cyan-100/85 font-semibold text-slate-900 shadow-none dark:bg-cyan-400/16 dark:text-white"
                  : "text-slate-700 hover:translate-x-1 hover:bg-white/78 hover:text-slate-900 dark:text-slate-100/88 dark:hover:bg-white/10",
              )}
              color={isActive ? "primary" : "default"}
              endContent={
                <div
                  className={clsx(
                    "ml-auto h-1.5 w-3 rounded-full transition-all duration-300",
                    isActive
                      ? "bg-cyan-400 shadow-lg shadow-cyan-300/35 dark:bg-cyan-300"
                      : "bg-slate-300 dark:bg-white/45",
                  )}
                  aria-hidden="true"
                />
              }
              startContent={item.icon}
              variant={isActive ? "shadow" : "light"}
              onPress={() => onNavigate(item.href)}
            >
              <span className="truncate text-sm font-semibold">{item.label}</span>
            </Button>
          </div>
        );
      })}
    </div>
  );
}
