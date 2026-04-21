import type { PropsWithChildren, ReactNode } from "react";

import { Card, CardBody, CardHeader } from "@heroui/card";

interface SectionSurfaceProps extends PropsWithChildren {
  id: string;
  title: string;
  description: string;
  actions?: ReactNode;
}

export default function SectionSurface({ id, title, description, actions, children }: SectionSurfaceProps) {
  return (
    <Card id={id} className="panel-surface scroll-mt-20 overflow-hidden rounded-2xl" shadow="none">
      <CardHeader className="flex flex-col items-start gap-3 p-5 md:flex-row md:items-start md:justify-between">
        <div className="flex flex-col gap-2">
          <div className="text-lg font-semibold text-slate-800 dark:text-white">{title}</div>
          <p className="m-0 max-w-3xl text-sm leading-6 text-slate-600 dark:text-white/82">{description}</p>
        </div>
        {actions ? <div className="shrink-0">{actions}</div> : null}
      </CardHeader>
      <CardBody className="pt-0">{children}</CardBody>
    </Card>
  );
}
