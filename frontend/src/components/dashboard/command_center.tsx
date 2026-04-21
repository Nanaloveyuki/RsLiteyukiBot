import { Card, CardBody } from "@heroui/card";

import type { CommandGroup } from "@/data/dashboard";

interface CommandCenterProps {
  groups: CommandGroup[];
  disabledCommands: string[];
}

export default function CommandCenter({ groups, disabledCommands }: CommandCenterProps) {
  return (
    <div className="grid gap-4">
      {groups.length ? (
        <div className="grid gap-4 xl:grid-cols-3">
          {groups.map((group) => (
            <Card key={group.id} className="panel-surface overflow-hidden rounded-[26px]" shadow="none">
              <CardBody className="gap-4 p-4">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <div className="text-xs font-semibold uppercase tracking-[0.2em] text-default-400 dark:text-slate-500">
                      {group.id}
                    </div>
                    <div className="mt-2 text-lg font-semibold text-default-800 dark:text-white">{group.title}</div>
                  </div>
                  <div className="rounded-full bg-primary-50 px-3 py-1 text-sm font-semibold text-primary-500 dark:bg-primary-500/15">
                    {group.commands.length}
                  </div>
                </div>
                <p className="m-0 text-sm leading-6 text-default-500 dark:text-slate-300">{group.description}</p>
                <div className="grid gap-2">
                  {group.commands.map((entry) => (
                    <div key={entry.name} className="rounded-[22px] bg-white/55 px-3 py-3 dark:bg-slate-900/45">
                      <code className="rounded-full bg-primary-50 px-2 py-1 text-xs text-primary-500 dark:bg-primary-500/15">
                        {entry.name}
                      </code>
                      <div className="mt-2 text-sm text-default-500 dark:text-slate-300">{entry.hint}</div>
                    </div>
                  ))}
                </div>
              </CardBody>
            </Card>
          ))}
        </div>
      ) : (
        <div className="text-sm text-default-400 dark:text-slate-500">没有匹配当前筛选条件的命令。</div>
      )}

      <Card className="panel-surface rounded-[26px]" shadow="none">
        <CardBody className="gap-3 p-4">
          <div className="text-sm font-semibold uppercase tracking-[0.2em] text-default-400 dark:text-slate-500">
            disabled commands
          </div>
          {disabledCommands.length ? (
            <div className="flex flex-wrap gap-2">
              {disabledCommands.map((entry) => (
                <code key={entry} className="rounded-full bg-danger-50 px-3 py-1 text-xs text-danger-500 dark:bg-danger-500/15">
                  {entry}
                </code>
              ))}
            </div>
          ) : (
            <div className="text-sm text-default-400 dark:text-slate-500">当前没有禁用命令。</div>
          )}
        </CardBody>
      </Card>
    </div>
  );
}
