import type { PropsWithChildren, ReactNode } from "react";

interface PageHeaderMetric {
  label: string;
  value: ReactNode;
}

interface PageHeaderProps extends PropsWithChildren {
  eyebrow: string;
  title: string;
  description: string;
  tags?: string[];
  metrics?: PageHeaderMetric[];
}

export default function PageHeader({
  eyebrow,
  title,
  description,
  tags,
  metrics,
  children,
}: PageHeaderProps) {
  return (
    <div className="panel-surface relative mb-5 overflow-hidden rounded-2xl">
      <div className="relative z-10 flex flex-col gap-5 p-5 md:p-6">
        <div className="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
          <div className="flex flex-col gap-3">
            <div className="text-xs font-semibold uppercase tracking-[0.3em] text-slate-500 dark:text-white/62">
              {eyebrow}
            </div>
            <div className="flex flex-col gap-2">
              <h1 className="m-0 text-3xl font-bold text-slate-800 dark:text-white md:text-4xl">{title}</h1>
              <p className="m-0 max-w-3xl text-sm leading-7 text-slate-600 dark:text-white/82">{description}</p>
            </div>
            {tags?.length ? (
              <div className="flex flex-wrap gap-2">
                {tags.map((tag, index) => (
                  <div key={`${tag}-${index}`} className="surface-chip max-w-full truncate text-slate-700 dark:text-white/84">
                    {tag}
                  </div>
                ))}
              </div>
            ) : null}
          </div>
          {children ? <div className="flex flex-wrap gap-3">{children}</div> : null}
        </div>
        {metrics?.length ? (
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
            {metrics.map((metric) => (
              <div
                key={metric.label}
                className="rounded-2xl border border-white/40 bg-white/65 px-4 py-3 backdrop-blur-sm dark:border-white/10 dark:bg-slate-950/38"
              >
                <div className="text-xs font-semibold uppercase tracking-[0.24em] text-slate-500 dark:text-white/58">
                  {metric.label}
                </div>
                <div className="mt-2 break-all text-lg font-semibold text-slate-800 dark:text-white">{metric.value}</div>
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}
