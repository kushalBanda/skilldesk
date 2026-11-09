import type { ReactNode } from "react";
import { t } from "../strings";
import { BackLink } from "./BackLink";

/**
 * Screen title row, the same on every screen: title (and an optional lead) on
 * the left; on the right, the screen's own search, or the ⌘K search where the
 * screen has no list to filter, then the screen's actions.
 */
export function PageHeader({ title, lead, search, children }: { title: string; lead?: ReactNode; search?: ReactNode; children?: ReactNode }) {
  return (
    <header className="flex flex-col gap-3">
      <BackLink />
      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <h1 className="m-0 flex-1 whitespace-nowrap text-[44px] font-bold leading-none tracking-[-0.05em]">{title}</h1>
        <div className="flex shrink-0 items-center gap-2.5">
          {search ?? <PaletteLauncher />}
          {children}
        </div>
      </div>
      {lead ? <p className="m-0 max-w-[72ch] text-[13.5px] text-ink-2">{lead}</p> : null}
    </header>
  );
}

/** Looks like the screens' search fields; opens the ⌘K palette. */
function PaletteLauncher() {
  return (
    <button
      type="button"
      onClick={() => window.dispatchEvent(new Event("skilldesk:palette"))}
      aria-keyshortcuts="Meta+K"
      className="flex h-9 w-64 items-center gap-2 rounded-full bg-sunk pl-3 pr-1.5 text-[13px] text-ink-3 ring-1 ring-transparent transition-[box-shadow] duration-150 hover:ring-rule active:scale-[0.98]"
    >
      <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
        <circle cx="7" cy="7" r="4.5" />
        <path d="M10.5 10.5L14 14" />
      </svg>
      <span className="flex-1 text-left">{t.paletteOpen}</span>
      <kbd className="mr-1 rounded-[6px] px-1.5 font-sans text-[11px] leading-[18px] text-ink-3 ring-1 ring-rule">⌘K</kbd>
    </button>
  );
}
