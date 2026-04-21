import { useState } from "react";

import { Card, CardBody } from "@heroui/card";

import PageHeader from "@/components/chrome/page_header";
import SegmentedControl from "@/components/chrome/segmented_control";
import NetworkItemDisplay from "@/components/dashboard/display_network_item";
import SectionSurface from "@/components/dashboard/section_surface";
import { adapterCapabilityNotes } from "@/data/dashboard";
import { useRuntimeHealthContext } from "@/contexts/runtime-health";
import { getBind, getDesktopUrl, getExternalUrlHint, getSuccessRate } from "@/lib/runtime-view";

export default function AdaptersPage() {
  const { health } = useRuntimeHealthContext();
  const runtime = health?.runtime;
  const bind = getBind(health);
  const desktopUrl = getDesktopUrl(health);
  const externalUrlHint = getExternalUrlHint(health);
  const successRate = getSuccessRate(runtime);
  const [panel, setPanel] = useState<"shared" | "capabilities">("shared");

  const stats = [
    { label: "bind", count: bind },
    { label: "desktop", count: desktopUrl },
    { label: "adapter count", count: runtime?.adapter_count ?? 0 },
    { label: "api requests", count: runtime?.external_stats.api_requests ?? 0 },
    { label: "success rate", count: `${successRate}%` },
    { label: "inflight", count: runtime?.external_stats.api_inflight ?? 0 },
  ];

  return (
    <section className="mx-auto w-full max-w-[1000px] overflow-hidden p-2 md:p-4">
      <PageHeader
        description="保留 NapCat 网络页的控制感，但把内容换成 RsLiteyukiBot 当前真实启用的共享入口、请求流量和适配器能力。"
        eyebrow="transport"
        metrics={[
          { label: "bind", value: bind },
          { label: "desktop", value: desktopUrl },
          { label: "success", value: `${successRate}%` },
          { label: "requests", value: runtime?.external_stats.api_requests ?? 0 },
        ]}
        tags={[externalUrlHint]}
        title="网络信息"
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
          id="adapter-capabilities"
          actions={
            <SegmentedControl
              items={[
                { key: "shared", label: "共享入口" },
                { key: "capabilities", label: "传输能力" },
              ]}
              selectedKey={panel}
              onChange={(key) => setPanel(key as typeof panel)}
            />
          }
          description="这些内容都来自同一个 health 快照，不额外发明前端侧 transport 状态。"
          title="网络能力"
        >
          {panel === "shared" ? (
            <div className="grid gap-4 lg:grid-cols-3">
              {[
                { title: "监听地址", value: bind, detail: "桌面端与 Web 端共用一个 host。" },
                { title: "桌面入口", value: desktopUrl, detail: "Tauri 壳默认回环地址。" },
                { title: "外部提示", value: externalUrlHint, detail: "对外提示地址，方便 Docker/Web 继续扩。" },
              ].map((entry) => (
                <Card key={entry.title} className="panel-surface rounded-2xl" shadow="none">
                  <CardBody className="gap-3 p-4">
                    <div className="text-sm font-semibold uppercase tracking-[0.22em] text-slate-500 dark:text-white/58">
                      {entry.title}
                    </div>
                    <div className="font-mono text-sm text-slate-800 dark:text-white">{entry.value}</div>
                    <p className="m-0 text-sm leading-6 text-slate-600 dark:text-white/88">{entry.detail}</p>
                  </CardBody>
                </Card>
              ))}
            </div>
          ) : null}

          {panel === "capabilities" ? (
            <div className="grid gap-4 lg:grid-cols-3">
              {adapterCapabilityNotes.map((entry) => (
                <Card key={entry.title} className="panel-surface rounded-2xl" shadow="none">
                  <CardBody className="gap-3 p-4">
                    <div className="text-lg font-semibold text-slate-800 dark:text-white">{entry.title}</div>
                    <p className="m-0 text-sm leading-6 text-slate-600 dark:text-white/88">{entry.detail}</p>
                  </CardBody>
                </Card>
              ))}
            </div>
          ) : null}
        </SectionSurface>

        <div className="grid gap-4 lg:grid-cols-2">
          <SectionSurface
            id="adapter-endpoints"
            description="共享 host 同时服务桌面端和浏览器，后续 Docker/Web 也走同一路径。"
            title="共享入口"
          >
            <div className="grid gap-3">
              <Card className="panel-surface rounded-[22px]" shadow="none">
                <CardBody className="gap-2 p-4 text-sm text-default-700 dark:text-slate-100/88">
                  <div className="font-semibold text-default-800 dark:text-white">桌面入口</div>
                  <div>{desktopUrl}</div>
                </CardBody>
              </Card>
              <Card className="panel-surface rounded-[22px]" shadow="none">
                <CardBody className="gap-2 p-4 text-sm text-default-700 dark:text-slate-100/88">
                  <div className="font-semibold text-default-800 dark:text-white">外部提示</div>
                  <div>{externalUrlHint}</div>
                </CardBody>
              </Card>
            </div>
          </SectionSurface>

          <SectionSurface
            id="adapter-api-stats"
            description="这里的流量统计来自同一个 runtime health 快照。"
            title="请求统计"
          >
            <div className="grid gap-3">
              <Card className="panel-surface rounded-[22px]" shadow="none">
                <CardBody className="grid gap-2 p-4 text-sm text-default-700 dark:text-slate-100/88 md:grid-cols-2">
                  <div>请求成功: {runtime?.external_stats.api_success ?? 0}</div>
                  <div>请求失败: {runtime?.external_stats.api_failed ?? 0}</div>
                  <div>超时次数: {runtime?.external_stats.api_timeouts ?? 0}</div>
                  <div>进行中: {runtime?.external_stats.api_inflight ?? 0}</div>
                </CardBody>
              </Card>
            </div>
          </SectionSurface>
        </div>
      </div>
    </section>
  );
}
