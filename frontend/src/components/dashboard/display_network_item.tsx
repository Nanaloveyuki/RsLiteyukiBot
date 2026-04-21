import { Card, CardBody } from "@heroui/card";
import clsx from "clsx";

interface NetworkItemDisplayProps {
  count: number | string;
  label: string;
  size?: "sm" | "md";
}

export default function NetworkItemDisplay({
  count,
  label,
  size = "md",
}: NetworkItemDisplayProps) {
  return (
    <Card
      className={clsx(
        "panel-surface group overflow-hidden rounded-2xl transition-all duration-300 hover:-translate-y-1 hover:bg-white/80 dark:hover:bg-slate-950/55",
        size === "md" ? "col-span-8 md:col-span-2" : "col-span-2 md:col-span-1",
      )}
      shadow="none"
    >
      <CardBody className="items-start p-3 md:gap-1 md:p-4">
        <div className="text-[10px] font-semibold uppercase tracking-[0.26em] text-slate-500 dark:text-white/58">
          {size === "md" ? "metric" : "node"}
        </div>
        <div
          className={clsx(
            "mt-2 flex-1 font-mono font-bold text-slate-800 dark:text-white",
            size === "md" ? "text-4xl md:text-5xl" : "text-2xl md:text-3xl",
          )}
        >
          {count}
        </div>
        <div
          className={clsx(
            "text-nowrap mt-2 shrink-0 whitespace-nowrap font-medium text-slate-600 dark:text-white/82",
            size === "md" ? "text-sm" : "text-xs",
          )}
        >
          {label}
        </div>
      </CardBody>
    </Card>
  );
}
