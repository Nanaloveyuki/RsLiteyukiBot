import clsx from "clsx";

import { useThemeMode } from "@/contexts/theme-mode";

interface UsagePieProps {
  systemUsage: number;
  appUsage: number;
  title: string;
  systemLabel: string;
  appLabel: string;
}

const CHART_COLORS = {
  system: "var(--chart-ring-system)",
  app: "var(--chart-ring-app)",
};

export default function UsagePie({
  systemUsage,
  appUsage,
  title,
  systemLabel,
  appLabel,
}: UsagePieProps) {
  const { isDark } = useThemeMode();
  const cleanSystem = Math.min(Math.max(systemUsage, 0), 100);
  const cleanApp = Math.min(Math.max(appUsage, 0), 100);
  const size = 100;
  const strokeWidth = 10;
  const radius = (size - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const center = size / 2;
  const systemDash = `${(cleanSystem / 100) * circumference} ${circumference}`;
  const appDash = `${(cleanApp / 100) * circumference} ${circumference}`;
  const trackColor = isDark ? "rgba(255,255,255,0.08)" : "rgba(15,23,42,0.07)";

  return (
    <div className="relative flex h-36 w-36 items-center justify-center">
      <svg className="h-full w-full -rotate-90" viewBox={`0 0 ${size} ${size}`}>
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke={trackColor}
          strokeLinecap="round"
          strokeWidth={strokeWidth}
        />
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke={CHART_COLORS.system}
          strokeDasharray={systemDash}
          strokeLinecap="round"
          strokeWidth={strokeWidth}
          className="transition-all duration-700 ease-out"
        />
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke={CHART_COLORS.app}
          strokeDasharray={appDash}
          strokeLinecap="round"
          strokeWidth={strokeWidth}
          className="transition-all duration-700 ease-out"
        />
      </svg>
      <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center select-none">
        <span className="mb-0.5 scale-90 text-[10px] font-medium uppercase tracking-widest text-slate-500 dark:text-white/55">
          {title}
        </span>
        <div className="flex items-baseline gap-0.5">
          <span className="font-mono text-2xl font-bold tracking-tight text-slate-900 dark:text-white">
            {Math.round(cleanSystem)}
          </span>
          <span className="text-xs font-bold text-slate-400 dark:text-white/42">%</span>
        </div>
        <div className="mt-1 flex flex-col items-center text-[10px] text-slate-400 dark:text-white/46">
          <span>{systemLabel}</span>
          <span className={clsx("font-medium text-sky-500 dark:text-cyan-200")}>{appLabel}</span>
        </div>
      </div>
    </div>
  );
}
