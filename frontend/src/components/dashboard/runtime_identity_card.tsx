import { Card, CardBody } from "@heroui/card";
import clsx from "clsx";
import { LuBot } from "react-icons/lu";

interface RuntimeIdentityCardProps {
  appName: string;
  runtimeTarget: string;
  status: string;
  updatedAt: string;
  error?: string;
  bind?: string;
  warnings: string[];
  notes: string[];
  lastEventTopic: string | null;
  pluginDirs: string[];
  disabledPlugins: string[];
}

export default function RuntimeIdentityCard({
  appName,
  runtimeTarget,
  status,
  updatedAt,
  error,
  bind,
  warnings,
  notes,
  lastEventTopic,
  pluginDirs,
  disabledPlugins,
}: RuntimeIdentityCardProps) {
  const toneClass =
    status === "running" ? "bg-success-500" : status === "starting" ? "bg-warning-400" : "bg-default-400";

  return (
    <Card className="panel-surface relative shrink-0 overflow-hidden rounded-2xl" shadow="none">
      <CardBody className="relative flex-row items-center gap-4 overflow-hidden p-5">
        <div className="absolute bottom-[-8px] right-2 text-[4.2rem] text-slate-300/22 dark:text-white/6">
          <LuBot />
        </div>
        <div className="relative z-10 shrink-0">
          <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-primary-400 via-primary-500 to-secondary-400 text-white shadow-lg shadow-primary-500/20">
            <LuBot className="text-[1.7rem]" />
          </div>
          <div className={clsx("absolute bottom-0.5 right-0.5 z-10 h-3.5 w-3.5 rounded-full border-2 border-white", toneClass)} />
        </div>
        <div className="z-10 flex-1">
          <div className="mb-1 flex flex-wrap items-center gap-2">
            <div className="truncate text-xl font-bold text-slate-800 dark:text-white">{appName}</div>
            <div className="rounded-full bg-white/68 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.2em] text-slate-600 dark:bg-slate-950/55 dark:text-white/76">
              {runtimeTarget}
            </div>
          </div>
          <div className="font-mono text-xs tracking-wider text-slate-600 opacity-80 dark:text-white/70">
            status · {status}
          </div>
          <div className="mt-2 text-xs text-slate-500 dark:text-white/58">{updatedAt || "waiting for snapshot"}</div>
          <div className="mt-3 rounded-2xl bg-white/48 px-3 py-3 text-sm text-slate-600 dark:bg-slate-950/42 dark:text-white/82">
            <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-slate-500 dark:text-white/58">
              共享地址
            </div>
            <div className="mt-2 font-mono text-xs">{bind ?? "0.0.0.0:14500"}</div>
          </div>
          <div className="mt-3 rounded-2xl bg-white/55 px-3 py-3 dark:bg-slate-950/48">
            <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-slate-500 dark:text-white/58">
              运行摘要
            </div>
            <div className="mt-3 grid gap-2">
              <div className="grid grid-cols-[4.5rem_1fr] gap-2 text-sm">
                <div className="font-medium text-slate-600 dark:text-white/74">最近事件</div>
                <div className="text-slate-700 dark:text-white/88">{lastEventTopic ?? "waiting for inbound events"}</div>
              </div>
              <div className="grid grid-cols-[4.5rem_1fr] gap-2 text-sm">
                <div className="font-medium text-slate-600 dark:text-white/74">告警</div>
                <div className="text-slate-700 dark:text-white/88">{warnings[0] ?? "当前无告警"}</div>
              </div>
              <div className="grid grid-cols-[4.5rem_1fr] gap-2 text-sm">
                <div className="font-medium text-slate-600 dark:text-white/74">备注</div>
                <div className="text-slate-700 dark:text-white/88">{notes[0] ?? "当前无备注"}</div>
              </div>
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <div className="rounded-full bg-white/74 px-3 py-1 text-xs text-slate-700 dark:bg-slate-900/55 dark:text-white/78">
                插件目录 {pluginDirs.length}
              </div>
              <div className="rounded-full bg-white/74 px-3 py-1 text-xs text-slate-700 dark:bg-slate-900/55 dark:text-white/78">
                禁用插件 {disabledPlugins.length}
              </div>
            </div>
          </div>
          {error ? <div className="mt-3 max-w-xs text-xs text-danger-500">{error}</div> : null}
        </div>
      </CardBody>
    </Card>
  );
}
