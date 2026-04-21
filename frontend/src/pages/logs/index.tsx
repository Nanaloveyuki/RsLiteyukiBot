import { Button } from "@heroui/button";
import { Card, CardBody } from "@heroui/card";
import { LuRefreshCcw } from "react-icons/lu";

import PageHeader from "@/components/chrome/page_header";
import SectionSurface from "@/components/dashboard/section_surface";
import { useRuntimeLogs } from "@/hooks/useRuntimeLogs";

export default function LogsPage() {
  const { entries, error, loading, refresh, updatedAt } = useRuntimeLogs();
  const latestEntry = entries.length ? entries[entries.length - 1] : undefined;

  return (
    <section className="mx-auto w-full max-w-[1000px] overflow-hidden p-2 md:p-4">
      <PageHeader
        description="日志页直接读取共享 runtime 的最近控制台输出，适合远程看启动、网络与运行异常，不再依赖本地窗口。"
        eyebrow="logs"
        metrics={[
          { label: "entries", value: entries.length },
          { label: "latest level", value: latestEntry?.level ?? (loading ? "loading" : "idle") },
          { label: "module", value: latestEntry?.module ?? "runtime" },
          { label: "updated", value: updatedAt || "waiting" },
        ]}
        title="日志信息"
      >
        <Button
          className="bg-primary-500 text-white shadow-lg shadow-primary-500/20"
          radius="full"
          startContent={<LuRefreshCcw size={16} />}
          onPress={() => void refresh()}
        >
          刷新
        </Button>
      </PageHeader>

      <SectionSurface
        id="runtime-logs"
        description="默认展示最近 200 条日志。当前优先覆盖统一 Logger 路径以及共享 host / Tauri 壳的关键运行日志。"
        title="控制台输出"
      >
        {error ? (
          <Card className="mb-4 rounded-2xl border border-danger-200 bg-danger-50/75 shadow-none dark:bg-danger-500/15" shadow="none">
            <CardBody className="p-4 text-sm text-danger-600 dark:text-danger-200">{error}</CardBody>
          </Card>
        ) : null}

        <div className="panel-surface rounded-2xl">
          <div className="hidden grid-cols-[10.5rem_5.5rem_minmax(9rem,14rem)_1fr] gap-3 border-b border-white/40 px-4 py-3 text-[11px] font-semibold uppercase tracking-[0.24em] text-slate-500 dark:border-white/10 dark:text-white/55 md:grid">
            <div>时间</div>
            <div>级别</div>
            <div>模块</div>
            <div>消息</div>
          </div>
          <div className="max-h-[32rem] overflow-y-auto">
            {entries.length ? (
              entries.map((entry, index) => (
                <div
                  key={`${entry.timestamp}-${entry.module}-${index}`}
                  className="grid gap-2 border-b border-white/28 px-4 py-3 text-sm text-slate-700 dark:border-white/6 dark:text-white/86 md:grid-cols-[10.5rem_5.5rem_minmax(9rem,14rem)_1fr] md:gap-3"
                >
                  <div className="font-mono text-xs text-slate-500 dark:text-white/58 md:text-xs">{entry.timestamp}</div>
                  <div className="font-mono text-xs font-semibold text-slate-700 dark:text-white/92">{entry.level}</div>
                  <div className="font-mono text-xs text-slate-600 dark:text-white/74">{entry.module}</div>
                  <div className="min-w-0 font-mono text-xs leading-6 text-slate-700 dark:text-white/86">{entry.message}</div>
                </div>
              ))
            ) : (
              <div className="px-4 py-12 text-center text-sm text-slate-500 dark:text-white/58">
                {loading ? "正在读取日志..." : "当前还没有可展示的日志。"}
              </div>
            )}
          </div>
        </div>
      </SectionSurface>
    </section>
  );
}
