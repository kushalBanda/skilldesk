import { useMemo, useRef, useState, type ReactNode } from "react";
import { NavLink, useNavigate } from "react-router-dom";
import { BrandLockup } from "./BrandMark";
import { nav, t } from "../strings";
import { useFixes } from "../ipc/queries";
import { ScanButton } from "./TopBar";
import { markSidebar } from "./BackLink";

// One glyph per screen: 16px, 1.5 stroke, drawn on the same grid as the rest of the app's icons.
const ICONS: Record<string, ReactNode> = {
  "/": (
    <>
      <rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1.2" />
      <rect x="9" y="2.5" width="4.5" height="4.5" rx="1.2" />
      <rect x="2.5" y="9" width="4.5" height="4.5" rx="1.2" />
      <rect x="9" y="9" width="4.5" height="4.5" rx="1.2" />
    </>
  ),
  "/fixes": (
    <>
      <path d="M8 2.5l5.8 10.2a.8.8 0 01-.7 1.2H2.9a.8.8 0 01-.7-1.2L8 2.5z" />
      <path d="M8 6.5v3M8 11.6v.1" />
    </>
  ),
  "/skills": <path d="M8 2l1.5 3.9L13.5 7.5 9.5 9.1 8 13l-1.5-3.9L2.5 7.5l4-1.6L8 2z" />,
  "/marketplace": (
    <>
      <path d="M3 5.5h10l-.8 7.6a1 1 0 01-1 .9H4.8a1 1 0 01-1-.9L3 5.5z" />
      <path d="M5.5 7V4.5a2.5 2.5 0 015 0V7" />
    </>
  ),
  "/plugins": (
    <>
      <path d="M6 2.5v3M10 2.5v3" />
      <path d="M4 5.5h8v2.5a4 4 0 01-8 0V5.5z" />
      <path d="M8 12v2" />
    </>
  ),
  "/hooks": (
    <>
      <path d="M10.5 2.5v7a3.25 3.25 0 01-6.5 0V8" />
      <path d="M2.5 9.5L4 8l1.5 1.5" />
    </>
  ),
  "/mcp": (
    <>
      <rect x="2.5" y="2.5" width="11" height="4.5" rx="1.2" />
      <rect x="2.5" y="9" width="11" height="4.5" rx="1.2" />
      <path d="M5 4.75h.1M5 11.25h.1" />
    </>
  ),
  "/subagents": (
    <>
      <circle cx="8" cy="4" r="1.75" />
      <circle cx="3.75" cy="12" r="1.75" />
      <circle cx="12.25" cy="12" r="1.75" />
      <path d="M7.1 5.5L4.6 10.5M8.9 5.5l2.5 5" />
    </>
  ),
  "/memory": (
    <>
      <path d="M3 3.5A1 1 0 014 2.5h7.5a1 1 0 011 1v10H4a1 1 0 01-1-1v-9z" />
      <path d="M3 12.5a1 1 0 011-1h8.5M6 5.5h4" />
    </>
  ),
  "/secrets": (
    <>
      <rect x="3" y="7" width="10" height="6.5" rx="1.5" />
      <path d="M5.5 7V5a2.5 2.5 0 015 0v2" />
    </>
  ),
  "/sessions": <path d="M2.5 4.5a2 2 0 012-2h7a2 2 0 012 2v5a2 2 0 01-2 2H7l-3 2.5v-2.5h0a1.5 1.5 0 01-1.5-1.5v-5.5z" />,
  "/cleanup": (
    <>
      <path d="M2.5 4.5h11M6.5 4.5V3a.5.5 0 01.5-.5h2a.5.5 0 01.5.5v1.5" />
      <path d="M4 4.5l.7 8.6a1 1 0 001 .9h4.6a1 1 0 001-.9l.7-8.6" />
    </>
  ),
  "/history": (
    <>
      <path d="M2.75 8a5.25 5.25 0 101.6-3.8" />
      <path d="M2.5 2.5v2.25h2.25M8 5v3l2 1.5" />
    </>
  ),
  "/settings": (
    <>
      <path d="M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6" />
      <circle cx="10" cy="4.5" r="1.5" />
      <circle cx="6" cy="11.5" r="1.5" />
    </>
  ),
};

function Glyph({ to }: { to: string }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className="shrink-0 text-ink-3 transition-colors duration-150 group-hover/nav:text-ink-2 group-aria-[current=page]/nav:text-accent"
    >
      {ICONS[to]}
    </svg>
  );
}

const row = ({ isActive }: { isActive: boolean }) =>
  [
    "group/nav flex items-center gap-2.5 rounded-input px-2.5 py-[5px] text-[14px] font-medium transition-colors duration-150",
    isActive ? "bg-accent-soft text-ink" : "text-ink-2 hover:bg-sunk active:bg-rule",
  ].join(" ");

const fold = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");

export function Sidebar() {
  const fixes = useFixes();
  const fixCount = fixes.data?.length ?? 0;
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const q = fold(query);
  const sections = useMemo(
    () => (q ? nav.map((s) => ({ ...s, items: s.items.filter((i) => fold(i.label).includes(q) || fold(s.group).includes(q)) })).filter((s) => s.items.length) : nav),
    [q],
  );
  const first = sections[0]?.items[0];

  return (
    <nav aria-label="Main" className="flex w-[248px] shrink-0 flex-col gap-3 scroll-quiet overflow-y-auto bg-sidebar px-3 pb-4 pt-11" data-tauri-drag-region>
      <div className="px-2.5 pb-1" data-tauri-drag-region>
        <BrandLockup size={22} />
      </div>
      <div
        role="search"
        className="group/filter flex h-8 items-center gap-2 rounded-input bg-sunk px-2.5 ring-1 ring-transparent transition-[box-shadow] duration-150 hover:ring-rule focus-within:ring-accent focus-within:shadow-[0_0_0_3px_var(--accent-soft)]"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true" className="shrink-0 text-ink-3">
          <path d="M2.5 4h11M4.5 8h7M6.5 12h3" />
        </svg>
        <input
          ref={input}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && query) {
              e.preventDefault();
              e.stopPropagation();
              setQuery("");
            } else if (e.key === "Enter" && first) {
              e.preventDefault();
              markSidebar();
              navigate(first.to);
              setQuery("");
              input.current?.blur();
            }
          }}
          placeholder={t.filterNav}
          aria-label={t.filterNav}
          spellCheck={false}
          autoCorrect="off"
          autoCapitalize="off"
          className="h-full min-w-0 flex-1 bg-transparent text-[13px] text-ink caret-accent outline-none placeholder:text-ink-3 focus-visible:outline-none"
        />
        {query ? (
          <button
            type="button"
            onClick={() => {
              setQuery("");
              input.current?.focus();
            }}
            aria-label={t.clearSearch}
            className="-mr-1 flex h-5 w-5 items-center justify-center rounded-full text-ink-3 hover:bg-rule hover:text-ink active:scale-90"
          >
            <svg width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6" />
            </svg>
          </button>
        ) : null}
      </div>
      {sections.length === 0 ? <p className="m-0 px-2.5 text-[12.5px] text-ink-3">{t.navNone}</p> : null}
      {sections.map((section) => (
        <div key={section.group} className="flex flex-col gap-0.5">
          <div className="px-2.5 pb-1 text-[12.5px] font-semibold text-ink-3">{section.group}</div>
          {section.items.map((item) => (
            <NavLink key={item.to} to={item.to} end={item.to === "/"} className={row} onClick={() => (markSidebar(), setQuery(""))}>
              <Glyph to={item.to} />
              <span className="flex-1 truncate">{item.label}</span>
              {item.to === "/fixes" && fixCount > 0 ? (
                <span className="tnum rounded-full bg-danger px-1.5 text-[11.5px] font-semibold leading-[18px] text-white">{fixCount}</span>
              ) : null}
            </NavLink>
          ))}
        </div>
      ))}
      <div className="mt-auto flex items-center gap-1">
        <NavLink to="/settings" title="⌘," onClick={markSidebar} className={(s) => row(s) + " flex-1"}>
          <Glyph to="/settings" />
          <span className="flex-1">{t.settingsNav}</span>
          <span className="text-[12px] text-ink-3">⌘,</span>
        </NavLink>
        <ScanButton />
      </div>
    </nav>
  );
}
