import type { ReactNode } from "react";

import { Card, CardBody, CardHeader } from "@heroui/card";
import { LuCircleEllipsis, LuCommand, LuLanguages, LuRocket, LuShield } from "react-icons/lu";

interface SystemInfoCardProps {
  locale: string;
  commandPrefix: string;
  whitelistSize: number;
  adapterAutostart: boolean;
  pluginDirCount: number;
  disabledCommandCount: number;
}

interface InfoItemProps {
  title: string;
  value: ReactNode;
  icon: ReactNode;
}

function InfoItem({ title, value, icon }: InfoItemProps) {
  return (
    <div className="flex items-baseline gap-3 py-2 text-sm text-slate-700 dark:text-white/88">
      <div className="self-center text-lg opacity-70">{icon}</div>
      <div className="w-24 font-medium">{title}</div>
      <div className="flex-1 font-mono text-xs text-slate-600 dark:text-white/72">{value}</div>
    </div>
  );
}

export default function SystemInfoCard({
  locale,
  commandPrefix,
  whitelistSize,
  adapterAutostart,
  pluginDirCount,
  disabledCommandCount,
}: SystemInfoCardProps) {
  return (
    <Card className="panel-surface flex-1 overflow-hidden rounded-2xl" shadow="none">
      <CardHeader className="items-center gap-2 px-5 pb-0 pt-5 font-bold text-slate-700 dark:text-white">
        <LuCircleEllipsis className="text-lg opacity-80" />
        <span>系统信息</span>
      </CardHeader>
      <CardBody className="flex-1 p-5 pt-3">
        <div className="flex h-full flex-col justify-between gap-2">
          <InfoItem icon={<LuLanguages />} title="语言环境" value={locale} />
          <InfoItem icon={<LuCommand />} title="命令前缀" value={commandPrefix} />
          <InfoItem icon={<LuShield />} title="帮助白名单" value={whitelistSize} />
          <InfoItem icon={<LuRocket />} title="自动启动" value={adapterAutostart ? "enabled" : "disabled"} />
          <InfoItem icon={<LuCircleEllipsis />} title="插件目录" value={pluginDirCount} />
          <InfoItem icon={<LuCommand />} title="禁用命令" value={disabledCommandCount} />
        </div>
      </CardBody>
    </Card>
  );
}
