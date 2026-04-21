import { LuSearch } from "react-icons/lu";

interface SearchFieldProps {
  value: string;
  placeholder: string;
  onChange: (value: string) => void;
}

export default function SearchField({ value, placeholder, onChange }: SearchFieldProps) {
  return (
    <label className="relative block min-w-[14rem]">
      <LuSearch className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-default-400" />
      <input
        className="h-11 w-full rounded-full border border-white/40 bg-white/65 pl-11 pr-4 text-sm text-default-700 outline-hidden transition-all duration-200 placeholder:text-default-400 focus:border-primary-300 focus:bg-white focus:ring-2 focus:ring-primary-100 dark:border-white/10 dark:bg-slate-900/50 dark:text-slate-100 dark:placeholder:text-slate-500 dark:focus:border-primary-500/50 dark:focus:bg-slate-900/75 dark:focus:ring-primary-500/15"
        placeholder={placeholder}
        type="text"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}
