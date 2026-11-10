// Back (DESIGN.md "Wayfinding"): a screen reached through a link inside the
// app (Fix list "Show me", a Dashboard tile, ⌘K) shows where it came
// from and goes back there with its filters and scroll as they were. The
// sidebar is a place to go, not a path, so its links never leave a Back.
import { useRef } from "react";
import { useLocation, useNavigate, useNavigationType } from "react-router-dom";
import { nav, t } from "../strings";

type From = { path: string; label: string };
const back = new Map<string, From>();
let viaSidebar = false;

/** Called by sidebar links: the next screen starts a new path. */
export const markSidebar = () => {
  viaSidebar = true;
};

const labelOf = (pathname: string) =>
  pathname === "/settings" ? t.settingsNav : (nav.flatMap((s) => s.items).find((i) => i.to === pathname)?.label ?? t.back);

/** Mounted once, above the routes: records where each in-app visit came from (during render, so screens see it in the same pass). */
export function useTrackBack() {
  const loc = useLocation();
  const type = useNavigationType();
  const prev = useRef<{ key: string; pathname: string; search: string } | null>(null);
  const p = prev.current;
  if (!p || p.key !== loc.key) {
    if (p && !back.has(loc.key)) {
      if (p.pathname === loc.pathname) {
        // Same screen, new filters: keep the way back it had.
        const keep = back.get(p.key);
        if (keep) back.set(loc.key, keep);
      } else if (!viaSidebar && type !== "POP") {
        back.set(loc.key, { path: p.pathname + p.search, label: labelOf(p.pathname) });
      }
    }
    viaSidebar = false;
    prev.current = { key: loc.key, pathname: loc.pathname, search: loc.search };
  }
}

/** "‹ Fix list": goes back one step. `fallback` is for screens that are always one level down. */
export function BackLink({ fallback }: { fallback?: { to: string; label: string } }) {
  const loc = useLocation();
  const navigate = useNavigate();
  const from = back.get(loc.key);
  const target = from ?? (fallback ? { path: fallback.to, label: fallback.label } : null);
  if (!target) return null;
  return (
    <button
      type="button"
      onClick={() => (from ? navigate(-1) : navigate(target.path))}
      aria-label={t.backTo(target.label)}
      className="group/back -ml-2 inline-flex h-7 items-center gap-1 self-start rounded-full pl-1.5 pr-3 text-[13px] font-medium text-ink-2 transition-colors duration-150 hover:bg-sunk hover:text-ink active:scale-[0.97]"
    >
      <svg
        width="14"
        height="14"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        className="transition-transform duration-150 group-hover/back:-translate-x-0.5"
      >
        <path d="M10 3.5L5.5 8l4.5 4.5" />
      </svg>
      {target.label}
    </button>
  );
}
