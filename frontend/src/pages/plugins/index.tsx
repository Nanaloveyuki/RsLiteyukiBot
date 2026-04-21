import { useDeferredValue, useMemo, useState } from "react";

import { Card, CardBody } from "@heroui/card";

import PageHeader from "@/components/chrome/page_header";
import SearchField from "@/components/chrome/search_field";
import SegmentedControl from "@/components/chrome/segmented_control";
import SectionSurface from "@/components/dashboard/section_surface";
import { useRuntimeHealthContext } from "@/contexts/runtime-health";
import { getDisabledPlugins, getNotes, getPluginDirs } from "@/lib/runtime-view";

export default function PluginsPage() {
  const { health } = useRuntimeHealthContext();
  const runtime = health?.runtime;
  const pluginDirs = getPluginDirs(runtime);
  const disabledPlugins = getDisabledPlugins(runtime);
  const notes = getNotes(runtime);
  const [query, setQuery] = useState("");
  const [panel, setPanel] = useState<"dirs" | "disabled" | "notes">("dirs");
  const deferredQuery = useDeferredValue(query.trim().toLowerCase());
  const filteredPluginDirs = useMemo(
    () => pluginDirs.filter((entry) => !deferredQuery || entry.toLowerCase().includes(deferredQuery)),
    [deferredQuery, pluginDirs],
  );
  const filteredDisabled = useMemo(
    () => disabledPlugins.filter((entry) => !deferredQuery || entry.toLowerCase().includes(deferredQuery)),
    [deferredQuery, disabledPlugins],
  );
  const filteredNotes = useMemo(
    () => notes.filter((entry) => !deferredQuery || entry.toLowerCase().includes(deferredQuery)),
    [deferredQuery, notes],
  );

  return (
    <section className="mx-auto w-full max-w-[1000px] overflow-hidden p-2 md:p-4">
      <PageHeader
        description="插件页保留 NapCat 的管理台结构，但内容严格依赖当前 runtime 已真实暴露出来的目录、禁用列表和运行笔记。"
        eyebrow="plugins"
        metrics={[
          { label: "dirs", value: pluginDirs.length },
          { label: "disabled", value: disabledPlugins.length },
          { label: "notes", value: notes.length },
          { label: "runtime", value: runtime?.status ?? "unknown" },
        ]}
        tags={pluginDirs.slice(0, 2)}
        title="插件面板"
      />

      <div className="flex flex-col gap-4">
        <SectionSurface
          id="plugin-directories"
          actions={
            <div className="flex flex-wrap items-center gap-3">
              <SegmentedControl
                items={[
                  { key: "dirs", label: "目录" },
                  { key: "disabled", label: "禁用" },
                  { key: "notes", label: "笔记" },
                ]}
                selectedKey={panel}
                onChange={(key) => setPanel(key as typeof panel)}
              />
              <SearchField placeholder="搜索目录、插件或备注" value={query} onChange={setQuery} />
            </div>
          }
          description="所有目录都直接来自 health 快照，保持桌面端、网页端和后续 Docker 端读到同一结果。"
          title="Plugin Directories"
        >
          {panel === "dirs" ? (
            filteredPluginDirs.length ? (
              <div className="flex flex-wrap gap-2">
                {filteredPluginDirs.map((entry) => (
                  <code key={entry} className="rounded-full bg-secondary-50 px-3 py-1 text-xs text-secondary-700 dark:bg-secondary-500/15">
                    {entry}
                  </code>
                ))}
              </div>
            ) : (
              <div className="text-sm text-default-400 dark:text-slate-500">当前未上报 plugin 目录。</div>
            )
          ) : null}

          {panel === "disabled" ? (
            filteredDisabled.length ? (
              <div className="flex flex-wrap gap-2">
                {filteredDisabled.map((entry) => (
                  <code key={entry} className="rounded-full bg-danger-50 px-3 py-1 text-xs text-danger-500 dark:bg-danger-500/15">
                    {entry}
                  </code>
                ))}
              </div>
            ) : (
              <div className="text-sm text-default-400 dark:text-slate-500">当前没有禁用插件。</div>
            )
          ) : null}

          {panel === "notes" ? (
            filteredNotes.length ? (
              <div className="grid gap-3">
                {filteredNotes.map((entry) => (
                  <div
                    key={entry}
                    className="rounded-[20px] bg-white/55 px-3 py-3 text-sm leading-6 text-default-500 dark:bg-slate-900/45 dark:text-slate-300"
                  >
                    {entry}
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-sm text-default-400 dark:text-slate-500">当前没有插件运行笔记。</div>
            )
          ) : null}
        </SectionSurface>

        <div className="grid gap-4 lg:grid-cols-2">
          <SectionSurface
            id="disabled-plugins"
            description="禁用状态仍由后端管理，这里只做读取和展示。"
            title="Disabled Plugins"
          >
            <div className="flex flex-wrap gap-2">
              {disabledPlugins.length ? (
                disabledPlugins.map((entry) => (
                  <code key={entry} className="rounded-full bg-danger-50 px-3 py-1 text-xs text-danger-500 dark:bg-danger-500/15">
                    {entry}
                  </code>
                ))
              ) : (
                <div className="text-sm text-default-400 dark:text-slate-500">当前没有禁用插件。</div>
              )}
            </div>
          </SectionSurface>

          <SectionSurface
            id="plugin-runtime-notes"
            description="插件相关运行笔记先借这一页暴露出来，后面可以继续扩成真实插件详情页。"
            title="Runtime Notes"
          >
            <div className="grid gap-3">
              {notes.length ? (
                notes.map((entry) => (
                  <Card key={entry} className="panel-surface rounded-[22px]" shadow="none">
                    <CardBody className="p-4 text-sm leading-6 text-default-500 dark:text-slate-300">{entry}</CardBody>
                  </Card>
                ))
              ) : (
                <div className="text-sm text-default-400 dark:text-slate-500">当前没有插件运行笔记。</div>
              )}
            </div>
          </SectionSurface>
        </div>
      </div>
    </section>
  );
}
