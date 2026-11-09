import { useEffect, useRef, useState } from "react";
// Agent marks (DESIGN.md "Agent marks"): the brand's own mark where one ships
// in `assets/agents` (lobe-icons, MIT; see NOTICE.md), else a two-letter monogram.
import type { AgentKey } from "../ipc/bindings/AgentKey";
import type { AgentSummary } from "../ipc/bindings/AgentSummary";
import { monogram } from "../lib/agents";

const MARKS = import.meta.glob<string>("../assets/agents/*.svg", { query: "?raw", import: "default", eager: true });
const BY_KEY = new Map(Object.entries(MARKS).map(([path, svg]) => [path.replace(/^.*\/(.+)\.svg$/, "$1"), svg]));

export function hasLogo(key: AgentKey): boolean {
  return BY_KEY.has(key);
}

/** The shared `~/.agents/skills` folder: a folder glyph, not a brand. */
function SharedGlyph({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" aria-hidden="true">
      <path d="M2 4.5A1.5 1.5 0 013.5 3h2.6l1.4 1.5h5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z" />
    </svg>
  );
}

/**
 * One agent's mark at `size` px. Decorative by default (the name sits next to
 * it); pass `label` when the mark stands alone, so it gets an accessible name.
 */
export function AgentLogo({
  agent,
  size = 16,
  label,
  plain,
  tone = "text-ink",
}: {
  agent: AgentSummary | AgentKey;
  size?: number;
  label?: string;
  /** Monogram without its own box (inside an AgentStack disc). */
  plain?: boolean;
  /** Color for single-color marks (default ink; "text-current" on an inverted chip). */
  tone?: string;
}) {
  const key = typeof agent === "string" ? agent : agent.key;
  const svg = BY_KEY.get(key);
  const a11y = label ? { role: "img" as const, "aria-label": label, title: label } : { "aria-hidden": true as const };
  if (key === "agents_shared") {
    return (
      <span {...a11y} className="inline-flex shrink-0 items-center justify-center text-ink-2" style={{ width: size, height: size }}>
        <SharedGlyph size={size} />
      </span>
    );
  }
  if (svg) {
    return (
      <span
        {...a11y}
        className={`agent-logo inline-flex shrink-0 items-center justify-center ${tone}`}
        style={{ width: size, height: size }}
        // Static SVG files bundled at build time (no user data).
        dangerouslySetInnerHTML={{ __html: svg }}
      />
    );
  }
  return (
    <span
      {...a11y}
      className={[
        "inline-flex shrink-0 items-center justify-center font-semibold leading-none tracking-[-0.02em] text-ink-2",
        plain ? "" : "rounded-[4px] bg-sunk ring-1 ring-rule",
      ].join(" ")}
      style={{ width: size, height: size, fontSize: Math.max(8, Math.round(size * (plain ? 0.62 : 0.5))) }}
    >
      {monogram(agent)}
    </span>
  );
}

/** Up to `max` marks, overlapping slightly, then "+N" (Skills, Plugins, MCP rows). */
export function AgentStack({
  agents,
  name,
  max = 5,
  size = 24,
  off,
  card = true,
}: {
  agents: AgentKey[];
  name: (key: AgentKey) => string;
  /** Agents where it is configured but switched off: shown faded. */
  off?: (key: AgentKey) => boolean;
  max?: number;
  size?: number;
  /** Hover card listing every agent (rows); off where the names show elsewhere. */
  card?: boolean;
}) {
  const shown = agents.slice(0, max);
  const rest = agents.length - shown.length;
  const label = (k: AgentKey) => (off?.(k) ? `${name(k)} (off)` : name(k));
  const [open, setOpen] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  // A short delay in, none out: the card does not flash while the pointer crosses rows.
  const enter = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(true), 180);
  };
  const leave = () => {
    window.clearTimeout(timer.current);
    setOpen(false);
  };
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return (
    <span className="relative inline-flex items-center" role="img" aria-label={agents.map(label).join(", ")} onPointerEnter={enter} onPointerLeave={leave}>
      {shown.map((k, i) => (
        <span
          key={k}
          className="relative grid place-items-center rounded-full bg-sunk ring-2 ring-bg"
          style={{ width: size, height: size, marginLeft: i === 0 ? 0 : -5, zIndex: max - i, opacity: off?.(k) ? 0.4 : 1 }}
        >
          <AgentLogo agent={k} size={Math.round(size * 0.6)} plain />
        </span>
      ))}
      {rest > 0 ? <span className="tnum ml-2 text-[12.5px] font-medium text-ink-2">+{rest}</span> : null}
      {card && open ? (
        <span
          role="tooltip"
          data-motion
          className="glass menu-in pointer-events-none absolute left-0 top-[calc(100%+8px)] z-30 flex w-max min-w-[200px] max-w-[280px] flex-col gap-0.5 rounded-row p-2 shadow-card ring-1 ring-rule"
        >
          <span className="px-1.5 pb-1 text-[11.5px] font-semibold text-ink-3">{agentsCount(agents.length)}</span>
          {agents.map((k) => (
            <span key={k} className={["flex items-center gap-2.5 rounded-input px-1.5 py-1 text-[13px]", off?.(k) ? "text-ink-3" : "text-ink"].join(" ")}>
              <AgentLogo agent={k} size={16} />
              <span className="truncate">{name(k)}</span>
              {off?.(k) ? <span className="ml-auto text-[11.5px] text-ink-3">Off</span> : null}
            </span>
          ))}
        </span>
      ) : null}
    </span>
  );
}

const agentsCount = (n: number) => `${n} ${n === 1 ? "agent" : "agents"}`;
