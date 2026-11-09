import type { ReactNode } from "react";
import { t } from "../strings";

/** Group header inside a table (project, agent, owner): [mark] name · count, click folds the group. */
export function GroupToggle({ label, count, open, onToggle, icon }: { label: string; count: number; open: boolean; onToggle: () => void; icon?: ReactNode }) {
  return (
    <button
      type="button"
      aria-expanded={open}
      aria-label={open ? t.collapseGroup(label) : t.expandGroup(label)}
      onClick={onToggle}
      className="group/gt -ml-1.5 inline-flex h-8 items-center gap-1.5 rounded-input px-1.5 text-[13px] font-semibold text-ink-2 hover:bg-sunk hover:text-ink active:scale-[0.98]"
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 12 12"
        aria-hidden="true"
        className={["shrink-0 text-ink-3 transition-transform duration-150", open ? "rotate-90" : ""].join(" ")}
      >
        <path d="M4.5 2.5L8 6l-3.5 3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      {icon}
      <span className="truncate">{label}</span>
      <span className="tnum font-normal text-ink-3">{count}</span>
    </button>
  );
}
