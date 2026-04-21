import { useState } from "react";

import { Card, CardBody } from "@heroui/card";

import PageHeader from "@/components/chrome/page_header";
import SegmentedControl from "@/components/chrome/segmented_control";
import NetworkItemDisplay from "@/components/dashboard/display_network_item";
import OperatorNotes from "@/components/dashboard/operator_notes";
import SectionSurface from "@/components/dashboard/section_surface";
import { useRuntimeHealthContext } from "@/contexts/runtime-health";
import {
  formatBytes,
  formatPercent,
  getDisabledPlugins,
  getNotes,
  getPluginDirs,
  getResourceUsage,
  getWarnings,
  hasResourceUsage,
} from "@/lib/runtime-view";

export default function DiagnosticsPage() {
  const { health, error } = useRuntimeHealthContext();
  const runtime = health?.runtime;
  const warnings = getWarnings(runtime);
  const notes = getNotes(runtime);
  const pluginDirs = getPluginDirs(runtime);
  const disabledPlugins = getDisabledPlugins(runtime);
  const resourceUsage = getResourceUsage(runtime);
  const usageReady = hasResourceUsage(resourceUsage);
  const [panel, setPanel] = useState<"overview" | "warnings" | "events">("overview");

  const stats = [
    { label: "warnings", count: warnings.length },
    { label: "notes", count: notes.length },
    { label: "CPU", count: usageReady ? formatPercent(resourceUsage?.cpu.system_percent) : "--" },
    { label: "memory", count: usageReady ? formatPercent(resourceUsage?.memory.system_percent) : "--" },
    { label: "events", count: runtime?.handled_events ?? 0 },
    { label: "last event", count: runtime?.last_event_topic ?? "idle" },
  ];

  return (
    <section className="mx-auto w-full max-w-[1000px] overflow-hidden p-2 md:p-4">
      <PageHeader
        description="诊断页保留 NapCat 的运维观察面板感，把告警、笔记、事件预览和资源快照重新组织成可快速扫读的结构。"
        eyebrow="diagnostics"
        metrics={[
          { label: "warnings", value: warnings.length },
          { label: "notes", value: notes.length },
          { label: "cpu", value: usageReady ? formatPercent(resourceUsage?.cpu.system_percent) : "--" },
          { label: "memory", value: usageReady ? formatPercent(resourceUsage?.memory.system_percent) : "--" },
        ]}
        tags={warnings.slice(0, 2)}
        title="诊断信息"
      />

      <div className="grid grid-cols-8 gap-x-1 gap-y-2 pb-5 md:grid-cols-3 md:gap-4 lg:grid-cols-6">
        {stats.map((item, index) => (
          <NetworkItemDisplay
            key={item.label}
            count={item.count}
            label={item.label}
            size={index < 2 ? "md" : "sm"}
          />
        ))}
      </div>

      <div className="flex flex-col gap-4">
        {error ? (
          <Card className="rounded-[24px] border border-danger-200 bg-danger-50/70 shadow-none dark:bg-danger-500/15">
            <CardBody className="p-4 text-sm text-danger-600">{error}</CardBody>
          </Card>
        ) : null}

        <SectionSurface
          id="operator-diagnostics"
          actions={
            <SegmentedControl
              items={[
                { key: "overview", label: "总览" },
                { key: "warnings", label: "告警" },
                { key: "events", label: "事件" },
              ]}
              selectedKey={panel}
              onChange={(key) => setPanel(key as typeof panel)}
            />
          }
          description="这里延续 NapCat 首页底部诊断卡片的风格，但内容全部替换成 RsLiteyukiBot 运行信息。"
          title="运行诊断"
        >
          {panel === "overview" ? (
            <OperatorNotes
              disabledPlugins={disabledPlugins}
              lastEventPreview={runtime?.last_event_preview ?? null}
              lastEventTopic={runtime?.last_event_topic ?? null}
              notes={notes}
              pluginDirs={pluginDirs}
              warnings={warnings}
            />
          ) : null}

          {panel === "warnings" ? (
            warnings.length ? (
              <div className="grid gap-3">
                {warnings.map((entry) => (
                  <div
                    key={entry}
                    className="rounded-[22px] bg-danger-50 px-4 py-3 text-sm leading-6 text-danger-500 dark:bg-danger-500/15"
                  >
                    {entry}
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-sm text-default-400 dark:text-slate-500">当前没有运行告警。</div>
            )
          ) : null}

          {panel === "events" ? (
            <div className="grid gap-4 lg:grid-cols-[1fr_0.9fr]">
              <Card className="panel-surface rounded-2xl" shadow="none">
                <CardBody className="gap-3 p-4">
                  <div className="text-sm font-semibold uppercase tracking-[0.22em] text-default-400 dark:text-slate-500">
                    最近事件
                  </div>
                  <div className="text-lg font-semibold text-slate-800 dark:text-white">
                    {runtime?.last_event_topic ?? "idle"}
                  </div>
                  <div className="rounded-2xl bg-white/58 px-3 py-3 text-sm leading-6 text-slate-500 dark:bg-slate-900/45 dark:text-white/72">
                    {runtime?.last_event_preview ?? "waiting for inbound events"}
                  </div>
                </CardBody>
              </Card>
              <Card className="panel-surface rounded-2xl" shadow="none">
                <CardBody className="gap-3 p-4 text-sm text-slate-500 dark:text-white/72">
                  <div className="text-sm font-semibold uppercase tracking-[0.22em] text-default-400 dark:text-slate-500">
                    资源快照
                  </div>
                  <div>System CPU: {usageReady ? formatPercent(resourceUsage?.cpu.system_percent) : "--"}</div>
                  <div>Process CPU: {usageReady ? formatPercent(resourceUsage?.cpu.process_percent) : "--"}</div>
                  <div>System Memory: {usageReady ? formatBytes(resourceUsage?.memory.used_bytes) : "--"}</div>
                  <div>Process Memory: {usageReady ? formatBytes(resourceUsage?.memory.process_bytes) : "--"}</div>
                  <div>handled events: {runtime?.handled_events ?? 0}</div>
                </CardBody>
              </Card>
            </div>
          ) : null}
        </SectionSurface>
      </div>
    </section>
  );
}
