import { Button } from "@heroui/button";
import { LuExternalLink, LuRefreshCcw } from "react-icons/lu";

import PageHeader from "@/components/chrome/page_header";
import NetworkItemDisplay from "@/components/dashboard/display_network_item";
import RuntimeIdentityCard from "@/components/dashboard/runtime_identity_card";
import SystemInfoCard from "@/components/dashboard/system_info_card";
import SystemStatusDisplay from "@/components/dashboard/system_status_display";
import { useRuntimeHealthContext } from "@/contexts/runtime-health";
import { resolveHealthUrl } from "@/lib/runtime";
import {
  formatBytes,
  formatPercent,
  getBind,
  getDesktopUrl,
  getDisabledCommands,
  getDisabledPlugins,
  getExternalUrlHint,
  getNotes,
  getPluginDirs,
  getResourceUsage,
  getWarnings,
  hasResourceUsage,
} from "@/lib/runtime-view";

export default function OverviewPage() {
  const { health, loading, error, refresh, updatedAt } = useRuntimeHealthContext();
  const runtime = health?.runtime;
  const bind = getBind(health);
  const desktopUrl = getDesktopUrl(health);
  const externalUrlHint = getExternalUrlHint(health);
  const warnings = getWarnings(runtime);
  const notes = getNotes(runtime);
  const disabledCommands = getDisabledCommands(runtime);
  const disabledPlugins = getDisabledPlugins(runtime);
  const pluginDirs = getPluginDirs(runtime);
  const resourceUsage = getResourceUsage(runtime);
  const usageReady = hasResourceUsage(resourceUsage);

  const overviewStats = [
    { label: "适配器实例", count: runtime?.adapter_count ?? 0 },
    { label: "事件数", count: runtime?.handled_events ?? 0 },
    { label: "CPU 总占用", count: usageReady ? formatPercent(resourceUsage?.cpu.system_percent) : "--" },
    { label: "进程 CPU", count: usageReady ? formatPercent(resourceUsage?.cpu.process_percent) : "--" },
    { label: "内存总占用", count: usageReady ? formatPercent(resourceUsage?.memory.system_percent) : "--" },
    { label: "进程内存", count: usageReady ? formatBytes(resourceUsage?.memory.process_bytes) : "--" },
  ];

  return (
    <section className="mx-auto w-full max-w-[1000px] overflow-hidden p-2 md:p-4">
      <PageHeader
        description="首页只保留真正有用的运行摘要，把共享入口和快捷入口从主视区移开，信息密度更接近 NapCat 的主面板。"
        eyebrow="dashboard"
        metrics={[
          { label: "runtime", value: runtime?.status ?? "unknown" },
          { label: "target", value: runtime?.runtime_target ?? "tauri2" },
          { label: "prefix", value: runtime?.llm_command_prefix ?? "/ask" },
          { label: "bind", value: bind },
        ]}
        tags={[externalUrlHint, desktopUrl]}
        title="基础信息"
      >
        <Button
          className="bg-primary-500 text-white shadow-lg shadow-primary-500/20"
          radius="full"
          startContent={<LuRefreshCcw size={16} />}
          onPress={() => void refresh()}
        >
          刷新
        </Button>
        <Button
          as="a"
          className="bg-white/70 text-default-700 dark:bg-slate-900/55 dark:text-slate-100"
          href={resolveHealthUrl()}
          radius="full"
          startContent={<LuExternalLink size={16} />}
          target="_blank"
          variant="flat"
        >
          健康接口
        </Button>
      </PageHeader>

      <div className="grid grid-cols-1 items-stretch gap-4 lg:grid-cols-3">
        <div className="flex flex-col gap-2">
          <RuntimeIdentityCard
            appName={runtime?.app_name ?? "RsLiteyukiBot"}
            bind={bind}
            disabledPlugins={disabledPlugins}
            error={error}
            lastEventTopic={runtime?.last_event_topic ?? null}
            notes={notes}
            pluginDirs={pluginDirs}
            runtimeTarget={runtime?.runtime_target ?? "tauri2"}
            status={loading ? "loading" : runtime?.status ?? "unknown"}
            updatedAt={updatedAt}
            warnings={warnings}
          />
          <SystemInfoCard
            adapterAutostart={runtime?.adapter_autostart ?? false}
            commandPrefix={runtime?.llm_command_prefix ?? "/ask"}
            disabledCommandCount={disabledCommands.length}
            locale={runtime?.locale ?? "zh-CN"}
            pluginDirCount={pluginDirs.length}
            whitelistSize={runtime?.help_whitelist_size ?? 0}
          />
        </div>
        <SystemStatusDisplay
          adapterCount={runtime?.adapter_count ?? 0}
          handledEvents={runtime?.handled_events ?? 0}
          resourceUsage={resourceUsage}
        />
      </div>

      <div className="grid grid-cols-8 gap-x-1 gap-y-2 py-5 md:grid-cols-3 md:gap-4 lg:grid-cols-6">
        {overviewStats.map((item, index) => (
          <NetworkItemDisplay
            key={item.label}
            count={item.count}
            label={item.label}
            size={index < 2 ? "md" : "sm"}
          />
        ))}
      </div>
    </section>
  );
}
