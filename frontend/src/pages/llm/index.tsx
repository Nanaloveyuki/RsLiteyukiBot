import { useState } from "react";

import { Card, CardBody } from "@heroui/card";

import PageHeader from "@/components/chrome/page_header";
import SegmentedControl from "@/components/chrome/segmented_control";
import NetworkItemDisplay from "@/components/dashboard/display_network_item";
import SectionSurface from "@/components/dashboard/section_surface";
import { commandGroups, llmCapabilityNotes } from "@/data/dashboard";
import { useRuntimeHealthContext } from "@/contexts/runtime-health";
import { getNotes } from "@/lib/runtime-view";

export default function LlmPage() {
  const { health } = useRuntimeHealthContext();
  const runtime = health?.runtime;
  const notes = getNotes(runtime);
  const sessionCommands = commandGroups.find((group) => group.id === "session");
  const [panel, setPanel] = useState<"entry" | "provider" | "prompt">("entry");

  const stats = [
    { label: "command prefix", count: runtime?.llm_command_prefix ?? "/ask" },
    { label: "help whitelist", count: runtime?.help_whitelist_size ?? 0 },
    { label: "handled events", count: runtime?.handled_events ?? 0 },
    { label: "notes", count: notes.length },
  ];

  return (
    <section className="mx-auto w-full max-w-[1000px] overflow-hidden p-2 md:p-4">
      <PageHeader
        description="按 NapCat 的能力页结构，把 /ask、provider 和 prompt profile 拆成面板式信息区，而不是只放说明文案。"
        eyebrow="llm"
        metrics={[
          { label: "prefix", value: runtime?.llm_command_prefix ?? "/ask" },
          { label: "whitelist", value: runtime?.help_whitelist_size ?? 0 },
          { label: "events", value: runtime?.handled_events ?? 0 },
          { label: "notes", value: notes.length },
        ]}
        tags={notes.slice(0, 2)}
        title="对话模式"
      />

      <div className="grid grid-cols-8 gap-x-1 gap-y-2 pb-5 md:grid-cols-2 md:gap-4 lg:grid-cols-4">
        {stats.map((item) => (
          <NetworkItemDisplay key={item.label} count={item.count} label={item.label} size="md" />
        ))}
      </div>

      <div className="flex flex-col gap-4">
        <SectionSurface
          id="llm-capabilities"
          actions={
            <SegmentedControl
              items={[
                { key: "entry", label: "入口" },
                { key: "provider", label: "Provider" },
                { key: "prompt", label: "Prompt" },
              ]}
              selectedKey={panel}
              onChange={(key) => setPanel(key as typeof panel)}
            />
          }
          description="这里不假装前端已经能编辑 provider，只把实际已存在的能力组织成更像控制台的视图。"
          title="LLM Capabilities"
        >
          <div className="grid gap-4 lg:grid-cols-3">
            {llmCapabilityNotes
              .filter((entry) => {
                if (panel === "entry") {
                  return entry.title === "Ask Entry";
                }
                if (panel === "provider") {
                  return entry.title === "Provider Routing";
                }

                return entry.title === "Prompt Store";
              })
              .map((entry) => (
                <Card key={entry.title} className="panel-surface rounded-[24px]" shadow="none">
                  <CardBody className="gap-3 p-4">
                    <div className="text-lg font-semibold text-default-800 dark:text-white">{entry.title}</div>
                    <p className="m-0 text-sm leading-6 text-default-500 dark:text-slate-300">{entry.detail}</p>
                  </CardBody>
                </Card>
              ))}
            <Card className="panel-surface rounded-[24px]" shadow="none">
              <CardBody className="gap-3 p-4">
                <div className="text-lg font-semibold text-default-800 dark:text-white">
                  {panel === "entry" ? "当前入口" : panel === "provider" ? "切换方式" : "配置存储"}
                </div>
                <div className="rounded-[18px] bg-white/55 px-3 py-3 text-sm leading-6 text-default-500 dark:bg-slate-900/45 dark:text-slate-300">
                  {panel === "entry"
                    ? "桌面端与 Web 端都依赖同一条 /ask 命令链路，当前前端先做可观测与导航，不自行发明第二套交互协议。"
                    : panel === "provider"
                      ? "Provider 仍由 /llm provider list|add|remove|use 控制，前端现在先把命令面板和状态摘要组织成可持续扩展的壳。"
                      : "Prompt profile 已经独立存储，这一页先保留 profile 视角和命令流程，后续再扩为真正的表单编辑。"}
                </div>
              </CardBody>
            </Card>
          </div>
        </SectionSurface>

        <SectionSurface
          id="llm-workflow"
          description="当前对话链路还没有完整搬到 Web，因此先用控制台卡片把真实命令路径和说明固定下来。"
          title="Conversation Workflow"
        >
          <div className="grid gap-4 lg:grid-cols-2">
            {sessionCommands?.commands.map((entry) => (
              <Card key={entry.name} className="panel-surface rounded-[24px]" shadow="none">
                <CardBody className="gap-3 p-4">
                  <code className="w-fit rounded-full bg-primary-50 px-3 py-1 text-xs text-primary-500 dark:bg-primary-500/15">
                    {entry.name}
                  </code>
                  <div className="text-sm leading-6 text-default-500 dark:text-slate-300">{entry.hint}</div>
                </CardBody>
              </Card>
            ))}
          </div>
        </SectionSurface>
      </div>
    </section>
  );
}
