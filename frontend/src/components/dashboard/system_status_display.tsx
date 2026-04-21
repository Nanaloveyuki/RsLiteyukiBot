import { Card, CardBody } from "@heroui/card";
import { LuCpu, LuLink, LuNetwork } from "react-icons/lu";

import { formatBytes, formatPercent, hasResourceUsage } from "@/lib/runtime-view";
import type { ResourceUsage } from "@/types/runtime";

import UsagePie from "./usage_pie";

interface SystemStatusDisplayProps {
  adapterCount: number;
  handledEvents: number;
  resourceUsage: ResourceUsage | null | undefined;
}

interface StatusItemProps {
  title: string;
  value: string | number;
  detail: string;
}

function StatusItem({ title, value, detail }: StatusItemProps) {
  return (
    <div className="rounded-2xl bg-white/62 px-3 py-3 dark:bg-slate-950/38">
      <div className="text-[11px] font-semibold uppercase tracking-[0.22em] text-slate-500 dark:text-white/55">
        {title}
      </div>
      <div className="mt-2 font-mono text-lg font-semibold text-slate-800 dark:text-white">{value}</div>
      <div className="mt-1 text-xs text-slate-500 dark:text-white/58">{detail}</div>
    </div>
  );
}

export default function SystemStatusDisplay({
  adapterCount,
  handledEvents,
  resourceUsage,
}: SystemStatusDisplayProps) {
  const usageReady = hasResourceUsage(resourceUsage);
  const systemCpu = usageReady ? formatPercent(resourceUsage?.cpu.system_percent) : "--";
  const processCpu = usageReady ? formatPercent(resourceUsage?.cpu.process_percent) : "--";
  const systemMemory = usageReady
    ? `${formatBytes(resourceUsage?.memory.used_bytes)} / ${formatBytes(resourceUsage?.memory.total_bytes)}`
    : "--";
  const processMemory = usageReady
    ? `${formatBytes(resourceUsage?.memory.process_bytes)} · ${formatPercent(resourceUsage?.memory.process_percent)}`
    : "--";

  return (
    <Card className="panel-surface relative col-span-1 overflow-hidden rounded-2xl lg:col-span-2" shadow="none">
      <CardBody className="z-10 gap-4 overflow-hidden p-5 lg:grid lg:grid-cols-[1fr_auto] lg:items-center">
        <div className="grid gap-5">
          <div>
            <h2 className="mb-3 flex items-center gap-2 text-lg font-semibold text-slate-800 dark:text-white">
              <LuCpu className="text-xl opacity-80" />
              <span>运行信息</span>
            </h2>
            <div className="grid gap-3 sm:grid-cols-2">
              <StatusItem title="系统 CPU" value={systemCpu} detail="当前整机总占用" />
              <StatusItem title="当前进程 CPU" value={processCpu} detail="RsLiteyukiBot 当前占用" />
              <StatusItem title="系统内存" value={systemMemory} detail="已用 / 总量" />
              <StatusItem title="当前进程内存" value={processMemory} detail="进程占用与占比" />
            </div>
          </div>

          <div>
            <h2 className="mb-3 flex items-center gap-2 text-lg font-semibold text-slate-800 dark:text-white">
              <LuNetwork className="text-xl opacity-80" />
              <span>网络信息</span>
            </h2>
            <div className="grid gap-3 sm:grid-cols-2">
              <StatusItem title="适配器" value={adapterCount} detail="当前接入实例数" />
              <StatusItem title="事件量" value={handledEvents} detail="累计处理事件" />
              <StatusItem title="资源采样" value={usageReady ? "已同步" : "等待采样"} detail="来源于共享 health 快照" />
              <StatusItem title="系统状态" value={usageReady ? "稳定" : "初始化"} detail="界面数据已转向用户可读指标" />
            </div>
          </div>
        </div>

        <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-1">
          <UsagePie
            appLabel="当前"
            appUsage={resourceUsage?.cpu.process_percent ?? 0}
            systemLabel="整机"
            systemUsage={resourceUsage?.cpu.system_percent ?? 0}
            title="CPU 使用率"
          />
          <UsagePie
            appLabel="当前"
            appUsage={resourceUsage?.memory.process_percent ?? 0}
            systemLabel="整机"
            systemUsage={resourceUsage?.memory.system_percent ?? 0}
            title="内存使用率"
          />
        </div>

        <div className="absolute right-5 top-5 text-[40px] text-cyan-100/80 dark:text-white/8">
          <LuLink />
        </div>
      </CardBody>
    </Card>
  );
}
