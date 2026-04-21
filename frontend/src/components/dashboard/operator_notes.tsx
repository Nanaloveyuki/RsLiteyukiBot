import { Card, CardBody } from "@heroui/card";

interface OperatorNotesProps {
  warnings: string[];
  notes: string[];
  lastEventTopic: string | null;
  lastEventPreview: string | null;
  pluginDirs: string[];
  disabledPlugins: string[];
}

function TextCollection({ title, entries }: { title: string; entries: string[] }) {
  return (
    <Card className="panel-surface rounded-[26px]" shadow="none">
      <CardBody className="gap-3 p-4">
        <div className="text-sm font-semibold uppercase tracking-[0.2em] text-default-400 dark:text-slate-500">{title}</div>
        {entries.length ? (
          <div className="grid gap-2">
            {entries.map((entry) => (
              <div
                key={entry}
                className="rounded-[20px] bg-white/55 px-3 py-3 text-sm leading-6 text-default-600 dark:bg-slate-900/45 dark:text-slate-300"
              >
                {entry}
              </div>
            ))}
          </div>
        ) : (
          <div className="text-sm text-default-400 dark:text-slate-500">当前无数据。</div>
        )}
      </CardBody>
    </Card>
  );
}

export default function OperatorNotes({
  warnings,
  notes,
  lastEventTopic,
  lastEventPreview,
  pluginDirs,
  disabledPlugins,
}: OperatorNotesProps) {
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <TextCollection entries={warnings} title="warnings" />
      <TextCollection entries={notes} title="notes" />
      <Card className="panel-surface rounded-[26px]" shadow="none">
        <CardBody className="gap-3 p-4">
          <div className="text-sm font-semibold uppercase tracking-[0.2em] text-default-400 dark:text-slate-500">last event</div>
          <div className="rounded-[20px] bg-white/55 px-3 py-3 dark:bg-slate-900/45">
            <div className="text-sm font-semibold text-default-800 dark:text-white">{lastEventTopic ?? "no event yet"}</div>
            <div className="mt-2 text-sm leading-6 text-default-500 dark:text-slate-300">
              {lastEventPreview ?? "waiting for inbound events"}
            </div>
          </div>
        </CardBody>
      </Card>
      <Card className="panel-surface rounded-[26px]" shadow="none">
        <CardBody className="gap-3 p-4">
          <div className="text-sm font-semibold uppercase tracking-[0.2em] text-default-400 dark:text-slate-500">plugins</div>
          {pluginDirs.length || disabledPlugins.length ? (
            <div className="flex flex-wrap gap-2">
              {pluginDirs.map((entry) => (
                <code key={entry} className="rounded-full bg-secondary-50 px-3 py-1 text-xs text-secondary-700 dark:bg-secondary-500/15">
                  {entry}
                </code>
              ))}
              {disabledPlugins.map((entry) => (
                <code key={entry} className="rounded-full bg-danger-50 px-3 py-1 text-xs text-danger-500 dark:bg-danger-500/15">
                  disabled: {entry}
                </code>
              ))}
            </div>
          ) : (
            <div className="text-sm text-default-400 dark:text-slate-500">当前未上报插件目录。</div>
          )}
        </CardBody>
      </Card>
    </div>
  );
}
