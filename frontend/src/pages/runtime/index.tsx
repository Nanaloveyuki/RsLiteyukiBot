import { useState } from "react";

import { Card, CardBody } from "@heroui/card";

import PageHeader from "@/components/chrome/page_header";
import SegmentedControl from "@/components/chrome/segmented_control";
import NetworkItemDisplay from "@/components/dashboard/display_network_item";
import SectionSurface from "@/components/dashboard/section_surface";
import { useRuntimeHealthContext } from "@/contexts/runtime-health";
import { getBind, getDesktopUrl, getExternalUrlHint, getNotes, getWarnings } from "@/lib/runtime-view";

export default function RuntimePage() {
  const { health, loading } = useRuntimeHealthContext();
  const runtime = health?.runtime;
  const bind = getBind(health);
  const desktopUrl = getDesktopUrl(health);
  const externalUrlHint = getExternalUrlHint(health);
  const notes = getNotes(runtime);
  const warnings = getWarnings(runtime);
  const [panel, setPanel] = useState<"overview" | "config" | "events">("overview");

  const stats = [
    { label: "状态", count: loading ? "loading" : runtime?.status ?? "unknown" },
    { label: "目标", count: runtime?.runtime_target ?? "tauri2" },
    { label: "语言环境", count: runtime?.locale ?? "zh-CN" },
    { label: "事件总量", count: runtime?.handled_events ?? 0 },
    { label: "帮助白名单", count: runtime?.help_whitelist_size ?? 0 },
    { label: "命令前缀", count: runtime?.llm_command_prefix ?? "/ask" },
  ];

  return (
    <section className="mx-auto w-full max-w-[1000px] overflow-hidden p-2 md:p-4">
      <PageHeader
        description="这里把 runtime 本体拆成 NapCat 式的概览区、配置区和事件区，避免把所有信息都挤在一张纯文本卡里。"
        eyebrow="runtime"
        metrics={[
          { label: "status", value: loading ? "loading" : runtime?.status ?? "unknown" },
          { label: "locale", value: runtime?.locale ?? "zh-CN" },
          { label: "events", value: runtime?.handled_events ?? 0 },
          { label: "whitelist", value: runtime?.help_whitelist_size ?? 0 },
        ]}
        tags={[bind, desktopUrl]}
        title="运行信息"
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
        <SectionSurface
          id="runtime-panels"
          actions={
            <SegmentedControl
              items={[
                { key: "overview", label: "概览" },
                { key: "config", label: "配置" },
                { key: "events", label: "事件" },
              ]}
              selectedKey={panel}
              onChange={(key) => setPanel(key as typeof panel)}
            />
          }
          description="切换不同视图时，只复用同一份后端快照，不额外复制状态。"
          title="Runtime Panels"
        >
          {panel === "overview" ? (
            <div className="grid gap-4 lg:grid-cols-2">
              <Card className="panel-surface rounded-[24px]" shadow="none">
                <CardBody className="gap-3 p-4">
                  <div className="text-sm font-semibold uppercase tracking-[0.22em] text-default-400 dark:text-slate-500">
                    lifecycle
                  </div>
                  <div className="text-2xl font-bold text-default-800 dark:text-white">
                    {runtime?.status ?? "unknown"}
                  </div>
                  <div className="grid gap-2 text-sm text-default-500 dark:text-slate-300">
                    <div>runtime target: {runtime?.runtime_target ?? "tauri2"}</div>
                    <div>llm prefix: {runtime?.llm_command_prefix ?? "/ask"}</div>
                    <div>handled events: {runtime?.handled_events ?? 0}</div>
                    <div>help whitelist: {runtime?.help_whitelist_size ?? 0}</div>
                  </div>
                </CardBody>
              </Card>
              <Card className="panel-surface rounded-[24px]" shadow="none">
                <CardBody className="gap-3 p-4">
                  <div className="text-sm font-semibold uppercase tracking-[0.22em] text-default-400 dark:text-slate-500">
                    entrypoints
                  </div>
                  <div className="grid gap-3 text-sm text-default-500 dark:text-slate-300">
                    <div className="rounded-[20px] bg-white/55 px-3 py-3 dark:bg-slate-900/45">
                      <div className="font-semibold text-default-800 dark:text-white">bind</div>
                      <div className="mt-2 font-mono text-xs">{bind}</div>
                    </div>
                    <div className="rounded-[20px] bg-white/55 px-3 py-3 dark:bg-slate-900/45">
                      <div className="font-semibold text-default-800 dark:text-white">desktop</div>
                      <div className="mt-2 font-mono text-xs">{desktopUrl}</div>
                    </div>
                    <div className="rounded-[20px] bg-white/55 px-3 py-3 dark:bg-slate-900/45">
                      <div className="font-semibold text-default-800 dark:text-white">external</div>
                      <div className="mt-2 font-mono text-xs">{externalUrlHint}</div>
                    </div>
                  </div>
                </CardBody>
              </Card>
            </div>
          ) : null}

          {panel === "config" ? (
            <Card className="rounded-[24px] border-0 bg-slate-950 shadow-none" shadow="none">
              <CardBody className="gap-3 p-4">
                <pre className="overflow-auto rounded-[20px] bg-slate-950 p-4 text-xs leading-6 text-slate-100">
                  {runtime?.runtime_config ?? "runtime config unavailable"}
                </pre>
              </CardBody>
            </Card>
          ) : null}

          {panel === "events" ? (
            <div className="grid gap-4 lg:grid-cols-2">
              <Card className="panel-surface rounded-[24px]" shadow="none">
                <CardBody className="gap-3 p-4">
                  <div className="text-sm font-semibold uppercase tracking-[0.22em] text-default-400 dark:text-slate-500">
                    last event
                  </div>
                  <div className="text-lg font-semibold text-default-800 dark:text-white">
                    {runtime?.last_event_topic ?? "no event yet"}
                  </div>
                  <div className="rounded-[20px] bg-white/55 px-3 py-3 text-sm leading-6 text-default-500 dark:bg-slate-900/45 dark:text-slate-300">
                    {runtime?.last_event_preview ?? "waiting for inbound events"}
                  </div>
                </CardBody>
              </Card>
              <div className="grid gap-4">
                <Card className="panel-surface rounded-[24px]" shadow="none">
                  <CardBody className="gap-3 p-4">
                    <div className="text-sm font-semibold uppercase tracking-[0.22em] text-default-400 dark:text-slate-500">
                      warnings
                    </div>
                    {warnings.length ? (
                      <div className="grid gap-2">
                        {warnings.map((entry) => (
                          <div
                            key={entry}
                            className="rounded-[18px] bg-danger-50 px-3 py-2 text-sm text-danger-500 dark:bg-danger-500/15"
                          >
                            {entry}
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="text-sm text-default-400 dark:text-slate-500">当前没有运行告警。</div>
                    )}
                  </CardBody>
                </Card>
                <Card className="panel-surface rounded-[24px]" shadow="none">
                  <CardBody className="gap-3 p-4">
                    <div className="text-sm font-semibold uppercase tracking-[0.22em] text-default-400 dark:text-slate-500">
                      notes
                    </div>
                    {notes.length ? (
                      <div className="grid gap-2">
                        {notes.map((entry) => (
                          <div
                            key={entry}
                            className="rounded-[18px] bg-white/55 px-3 py-2 text-sm text-default-500 dark:bg-slate-900/45 dark:text-slate-300"
                          >
                            {entry}
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="text-sm text-default-400 dark:text-slate-500">当前没有运行笔记。</div>
                    )}
                  </CardBody>
                </Card>
              </div>
            </div>
          ) : null}
        </SectionSurface>

        <div className="grid gap-4 lg:grid-cols-2">
          <SectionSurface
            id="runtime-entrypoints"
            description="共享 host 让桌面端和网页端读取同一套 runtime 数据。"
            title="Entrypoints"
          >
            <div className="grid gap-3">
              <Card className="panel-surface rounded-[22px]" shadow="none">
                <CardBody className="gap-2 p-4 text-sm text-default-600">
                  <div className="font-semibold text-default-800 dark:text-white">bind</div>
                  <div>{bind}</div>
                </CardBody>
              </Card>
              <Card className="panel-surface rounded-[22px]" shadow="none">
                <CardBody className="gap-2 p-4 text-sm text-default-600">
                  <div className="font-semibold text-default-800 dark:text-white">desktop</div>
                  <div>{desktopUrl}</div>
                </CardBody>
              </Card>
              <Card className="panel-surface rounded-[22px]" shadow="none">
                <CardBody className="gap-2 p-4 text-sm text-default-600">
                  <div className="font-semibold text-default-800 dark:text-white">external</div>
                  <div>{externalUrlHint}</div>
                </CardBody>
              </Card>
            </div>
          </SectionSurface>

          <SectionSurface
            id="runtime-event"
            description="最后一条事件对诊断 runtime 行为非常关键，因此单独抽成卡片。"
            title="Last Event"
          >
            <Card className="panel-surface rounded-[22px]" shadow="none">
              <CardBody className="gap-3 p-4">
                <div className="text-sm font-semibold uppercase tracking-[0.2em] text-default-400 dark:text-slate-500">
                  topic
                </div>
                <div className="text-lg font-semibold text-default-800 dark:text-white">
                  {runtime?.last_event_topic ?? "no event yet"}
                </div>
                <div className="rounded-[20px] bg-white/55 px-3 py-3 text-sm leading-6 text-default-500 dark:bg-slate-900/45 dark:text-slate-300">
                  {runtime?.last_event_preview ?? "waiting for inbound events"}
                </div>
              </CardBody>
            </Card>
          </SectionSurface>
        </div>
      </div>
    </section>
  );
}
