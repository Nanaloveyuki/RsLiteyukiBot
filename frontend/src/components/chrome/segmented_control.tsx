import clsx from "clsx";

export interface SegmentedControlItem {
  key: string;
  label: string;
}

interface SegmentedControlProps {
  items: SegmentedControlItem[];
  selectedKey: string;
  onChange: (key: string) => void;
}

export default function SegmentedControl({
  items,
  selectedKey,
  onChange,
}: SegmentedControlProps) {
  return (
    <div className="inline-flex flex-wrap gap-2 rounded-full border border-white/35 bg-white/45 p-1 backdrop-blur-md dark:border-white/10 dark:bg-slate-900/45">
      {items.map((item) => {
        const selected = item.key === selectedKey;

        return (
          <button
            key={item.key}
            className={clsx(
              "rounded-full px-4 py-2 text-sm font-medium transition-all duration-200",
              selected
                ? "bg-primary-500 text-white shadow-lg shadow-primary-500/20"
                : "text-default-700 hover:bg-white/70 hover:text-default-900 dark:text-slate-100/85 dark:hover:bg-white/10 dark:hover:text-white",
            )}
            type="button"
            onClick={() => onChange(item.key)}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
