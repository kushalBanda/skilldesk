// Numbers at the top of a screen (DESIGN.md §4 "Stat"): one grouped panel,
// hairline-divided tiles. Label above, big tabular number with its unit set
// smaller, one line of context under it. A tile that leads somewhere is a
// button: it darkens on hover, presses in, and shows a chevron.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";

export type StatItem = {
  label: string;
  /** Formatted value; a trailing unit ("122.7 min", "4.9 MB", "40 days") is set smaller. */
  value: string;
  hint?: ReactNode;
  tone?: "danger" | "warn" | null;
  /** 0–1: a thin bar under the number (share of a whole). */
  meter?: number;
  /** Extra content after the number (agent marks). */
  aside?: ReactNode;
  to?: string;
  onClick?: () => void;
  /** Pressed state for tiles that act as a filter. */
  active?: boolean;
  /** One whole split into parts (≤ 4): a thin segmented bar with a legend of names and counts. */
  split?: SplitPart[];
  /** Where the number comes from: up to 3 parts, largest first, as ranked bars on one baseline. */
  bars?: BarPart[];
};

export type Tone = "ok" | "warn" | "danger" | "accent" | "muted";
export type SplitPart = { label: string; value: number; tone: Tone };
export type BarPart = { label: string; value: number; display?: string; icon?: ReactNode };

const TONE: Record<Tone, string> = {
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
  accent: "var(--accent)",
  muted: "var(--ink-3)",
};

/** Thin segmented bar (8px, rounded ends, 2px gaps) and a one-line legend. */
function Split({ parts }: { parts: SplitPart[] }) {
  const shown = parts.filter((p) => p.value > 0);
  const total = shown.reduce((a, p) => a + p.value, 0);
  if (!total) return null;
  const name = parts.map((p) => `${p.label} ${p.value.toLocaleString("en-US")}`).join(", ");
  return (
    <span className="stat-extra mt-2.5 flex flex-col gap-1.5">
      <span role="img" aria-label={name} className="flex h-1.5 gap-[2px] overflow-hidden rounded-full">
        {shown.map((p) => (
          <span
            key={p.label}
            title={`${p.label}: ${p.value.toLocaleString("en-US")} (${Math.round((p.value / total) * 100)}%)`}
            className="h-full min-w-[3px] transition-[flex-grow] duration-500"
            style={{ flexGrow: p.value, flexBasis: 0, background: TONE[p.tone], opacity: p.tone === "muted" ? 0.45 : 1 }}
          />
        ))}
      </span>
      <span className="flex min-w-0 flex-wrap gap-x-3 gap-y-1 text-[12px] text-ink-3">
        {shown.map((p) => (
          <span key={p.label} className="inline-flex items-center gap-1.5 whitespace-nowrap">
            <i className="h-1.5 w-1.5 rounded-full" style={{ background: TONE[p.tone], opacity: p.tone === "muted" ? 0.45 : 1 }} />
            {p.label}
            <span className="tnum text-ink-2">{p.value.toLocaleString("en-US")}</span>
          </span>
        ))}
      </span>
    </span>
  );
}

/** Top parts as bars on one baseline: name, bar, value. One hue; the leader in full accent. */
function Bars({ parts }: { parts: BarPart[] }) {
  const top = [...parts]
    .filter((p) => p.value > 0)
    .sort((a, b) => b.value - a.value)
    .slice(0, 3);
  const max = top[0]?.value ?? 0;
  if (!max) return null;
  return (
    <span className="stat-extra mt-2.5 flex flex-col gap-1">
      {top.map((p, i) => (
        <span
          key={p.label}
          className="grid grid-cols-[minmax(0,1fr)_minmax(24px,30%)_2.75rem] items-center gap-2 text-[12px]"
          title={`${p.label}: ${p.display ?? p.value.toLocaleString("en-US")}`}
        >
          <span className="flex min-w-0 items-center gap-1.5 text-ink-2">
            {p.icon}
            <span className="truncate">{p.label}</span>
          </span>
          <span className="h-1.5 overflow-hidden rounded-full bg-rule-2">
            <span
              className="block h-full rounded-full transition-[width] duration-500"
              style={{ width: `${Math.max(3, (p.value / max) * 100)}%`, background: "var(--accent)", opacity: i === 0 ? 1 : 0.55 }}
            />
          </span>
          <span className="tnum text-right text-ink-3">{p.display ?? p.value.toLocaleString("en-US")}</span>
        </span>
      ))}
    </span>
  );
}

const reduced = () => typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** "8,822" → counts up once from 0 (600ms, ease-out); units and decimals keep their format. */
function useCountUp(text: string): string {
  const m = /^(\d[\d,]*)(\.\d+)?/.exec(text);
  const target = m ? Number((m[1] ?? "").replace(/,/g, "") + (m[2] ?? "")) : NaN;
  const decimals = m?.[2] ? m[2].length - 1 : 0;
  const played = useRef(false);
  const [shown, setShown] = useState<number | null>(null);
  useEffect(() => {
    if (played.current || !Number.isFinite(target) || target < 10 || reduced()) return;
    played.current = true;
    let raf = 0;
    const start = performance.now();
    const tick = (now: number) => {
      const p = Math.min(1, (now - start) / 600);
      const eased = 1 - Math.pow(1 - p, 3);
      setShown(p >= 1 ? null : target * eased);
      if (p < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [target]);
  if (shown === null || !m) return text;
  const n = shown.toLocaleString("en-US", { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
  return n + text.slice(m[0].length);
}

function Value({ text, tone }: { text: string; tone?: StatItem["tone"] }) {
  const live = useCountUp(text);
  const m = /^([^\s]+)\s+(.+)$/.exec(live);
  const color = tone === "danger" ? "text-danger" : tone === "warn" ? "text-warn-text" : "text-ink";
  return (
    <span className={["tnum flex items-baseline gap-1.5 leading-none", color].join(" ")}>
      <span className="text-[24px] font-semibold tracking-[-0.03em]">{m ? m[1] : live}</span>
      {m ? <span className="text-[13px] font-medium text-ink-3">{m[2]}</span> : null}
    </span>
  );
}

function Tile({ s }: { s: StatItem }) {
  const navigate = useNavigate();
  const go = s.onClick ?? (s.to ? () => navigate(s.to!) : undefined);
  const body = (
    <>
      <span className="flex min-w-0 items-center gap-1.5 truncate whitespace-nowrap text-[12.5px] font-medium text-ink-3">
        {s.tone ? <i className={["h-1.5 w-1.5 rounded-full", s.tone === "danger" ? "bg-danger" : "bg-warn-text"].join(" ")} /> : null}
        {s.label}
        {go ? (
          <svg
            width="12"
            height="12"
            viewBox="0 0 12 12"
            aria-hidden="true"
            className="ml-auto opacity-0 transition-opacity duration-150 group-hover/tile:opacity-60"
          >
            <path d="M4.5 2.5L8 6l-3.5 3.5" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        ) : null}
      </span>
      <span className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-2">
        <Value text={s.value} tone={s.tone} />
        {s.aside}
      </span>
      {s.meter !== undefined ? (
        <span className="mt-2.5 block h-1 overflow-hidden rounded-full bg-rule-2">
          <span
            className="block h-full rounded-full bg-accent transition-[width] duration-500"
            style={{ width: `${Math.max(0, Math.min(1, s.meter)) * 100}%` }}
          />
        </span>
      ) : null}
      {s.hint ? <span className="stat-hint mt-1.5 line-clamp-2 block text-[12px] leading-snug text-ink-3">{s.hint}</span> : null}
      {s.split ? <Split parts={s.split} /> : null}
      {s.bars ? <Bars parts={s.bars} /> : null}
    </>
  );
  const base = "group/tile flex min-w-0 flex-col px-4 py-3.5 text-left";
  if (!go) return <div className={base}>{body}</div>;
  return (
    <button
      type="button"
      onClick={go}
      aria-pressed={s.active}
      className={[
        base,
        "transition-[background-color,transform] duration-100 hover:bg-panel active:scale-[0.985] active:bg-sunk",
        s.active ? "bg-accent-soft hover:bg-accent-soft" : "",
      ].join(" ")}
    >
      {body}
    </button>
  );
}

export function StatTiles({ items, label }: { items: StatItem[]; label?: string; min?: number }) {
  // Tiles flow into as many columns as fit (app.css `.stat-tiles`), so the
  // strip reads the same on a full-screen window and a narrow one.
  return (
    <section
      aria-label={label}
      className="stat-tiles grid shrink-0 overflow-hidden rounded-card bg-bg ring-1 ring-rule"
    >
      {items.map((s) => (
        <Tile key={s.label} s={s} />
      ))}
    </section>
  );
}
