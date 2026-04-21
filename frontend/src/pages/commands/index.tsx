import { useDeferredValue, useMemo, useState } from "react";

import { Card, CardBody } from "@heroui/card";

import PageHeader from "@/components/chrome/page_header";
import SearchField from "@/components/chrome/search_field";
import CommandCenter from "@/components/dashboard/command_center";
import SectionSurface from "@/components/dashboard/section_surface";
import { commandGroups } from "@/data/dashboard";
import { useRuntimeHealthContext } from "@/contexts/runtime-health";
import { getDisabledCommands } from "@/lib/runtime-view";

export default function CommandsPage() {
  const { health } = useRuntimeHealthContext();
  const runtime = health?.runtime;
  const disabledCommands = getDisabledCommands(runtime);
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query.trim().toLowerCase());
  const filteredGroups = useMemo(() => {
    if (!deferredQuery) {
      return commandGroups;
    }

    return commandGroups
      .map((group) => ({
        ...group,
        commands: group.commands.filter(
          (entry) =>
            entry.name.toLowerCase().includes(deferredQuery) || entry.hint.toLowerCase().includes(deferredQuery),
        ),
      }))
      .filter(
        (group) =>
          group.title.toLowerCase().includes(deferredQuery) ||
          group.description.toLowerCase().includes(deferredQuery) ||
          group.commands.length > 0,
      );
  }, [deferredQuery]);

  return (
    <section className="mx-auto w-full max-w-[1000px] overflow-hidden p-2 md:p-4">
      <PageHeader
        description="命令页按 NapCat 的模块面板思路重做，同时加上本地过滤控件，方便快速定位 builtin command 与 policy 语义。"
        eyebrow="commands"
        metrics={[
          { label: "groups", value: commandGroups.length },
          { label: "disabled", value: disabledCommands.length },
          { label: "prefix", value: runtime?.llm_command_prefix ?? "/ask" },
          { label: "whitelist", value: runtime?.help_whitelist_size ?? 0 },
        ]}
        tags={disabledCommands.slice(0, 2)}
        title="命令中心"
      />

      <div className="flex flex-col gap-4">
        <SectionSurface
          id="command-groups"
          actions={<SearchField placeholder="搜索命令或说明" value={query} onChange={setQuery} />}
          description="命令以真实功能边界分组，后续如果你要做命令开关页，可以直接在这套面板结构上继续扩。"
          title="Command Groups"
        >
          <CommandCenter disabledCommands={disabledCommands} groups={filteredGroups} />
        </SectionSurface>

        <SectionSurface
          id="command-policy"
          description="命令策略仍由后端控制，这里暂时只展示当前状态而不发明一套前端编辑器。"
          title="Policy Notes"
        >
          <Card className="panel-surface rounded-[24px]" shadow="none">
            <CardBody className="gap-3 p-4 text-sm leading-6 text-default-500 dark:text-slate-300">
              <div>
                当前命令策略支持按 scope 做 enable / disable，并且与 /help 白名单控制保持独立。
              </div>
              <div>
                等你后面确认要把策略编辑搬到 Web，这一页可以直接扩展成 NapCat 式配置面板。
              </div>
            </CardBody>
          </Card>
        </SectionSurface>
      </div>
    </section>
  );
}
