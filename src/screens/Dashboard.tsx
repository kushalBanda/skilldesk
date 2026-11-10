import { Menu } from "../components/Menu";
import { Th, sortBy, useSort } from "../components/SortHeader";
import { PageHeader } from "../components/PageHeader";
import { AgentLogo, AgentStack } from "../components/AgentLogo";
import { StatTiles, type StatItem } from "../components/StatTiles";
import type React from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import type { Overview } from "../ipc/bindings/Overview";
import type { Fix } from "../ipc/bindings/Fix";
import type { Item } from "../ipc/bindings/Item";
import type { ItemKind } from "../ipc/bindings/ItemKind";
import type { ScanStage } from "../ipc/bindings/ScanStage";
import { getErrorMessage } from "../ipc/error";
import { useFixes, useItems, useScanStages, useScanning } from "../ipc/queries";
import { revealSections, sweepGrid } from "../lib/motion";
import { itemRoute, withParams } from "../lib/routes";
import { formatCount } from "../lib/format";
import { agentColor } from "../lib/series";
import { urgency } from "./Fixes";
import { kindLabel, t } from "../strings";

type Props = { data: Overview | undefined; error: unknown; loading: boolean; reveal?: boolean };

const MON: readonly string[] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DOW: readonly string[] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const RANGE_DAYS = [0, 365, 90, 30];
const CHIPS_SHOWN = 3;
const LIST_ROWS = 5;

/** A local calendar day. */
type Day = { key: string; date: Date; dow: number; counts: Record<string, number>; total: number };

const pad2 = (n: number) => String(n).padStart(2, "0");
const dayKey = (d: Date) => `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
const longDay = (d: Date) => `${DOW[(d.getDay() + 6) % 7]} ${d.getDate()} ${MON[d.getMonth()]} ${d.getFullYear()}`;
const hourName = (h: number) => (h === 0 ? "12 AM" : h < 12 ? `${h} AM` : h === 12 ? "12 PM" : `${h - 12} PM`);
const level = (v: number) => (v === 0 ? 0 : v <= 5 ? 1 : v <= 15 ? 2 : v <= 35 ? 3 : v <= 70 ? 4 : 5);

/** Every day from the first prompt (or the range start) to today, oldest first. */
function buildDays(o: Overview, range: number): Day[] {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const byDay = new Map<string, Record<string, number>>();
  for (const a of o.activity) {
    const m = byDay.get(a.day) ?? {};
    m[a.agent] = (m[a.agent] ?? 0) + a.prompts;
    byDay.set(a.day, m);
  }
  let start: Date;
  if (RANGE_DAYS[range]) {
    start = new Date(today);
    start.setDate(start.getDate() - RANGE_DAYS[range] + 1);
  } else {
    const first = o.activity[0]?.day;
    start = first ? new Date(`${first}T00:00:00`) : new Date(today);
  }
  const out: Day[] = [];
  for (const d = new Date(start); d <= today; d.setDate(d.getDate() + 1)) {
    const key = dayKey(d);
    const counts = byDay.get(key) ?? {};
    out.push({ key, date: new Date(d), dow: (d.getDay() + 6) % 7, counts, total: 0 });
  }
  return out;
}

export function Dashboard({ data, error, loading, reveal = false }: Props) {
  const [range, setRange] = useState(0);
  const [agent, setAgent] = useState<string | null>(null);
  const [moreChips, setMoreChips] = useState(false);
  const scanning = useScanning();
  const body = useRef<HTMLElement>(null);
  const hasPrompts = !!data?.activity.length;
  // First run: as the discovery scene lifts away, the sections rise in and
  // the activity grid wipes in (lib/motion "Act 3"). Once only.
  const [sweep, setSweep] = useState(false);
  const revealed = useRef(false);
  useEffect(() => {
    if (!reveal || revealed.current) return;
    revealed.current = true;
    if (hasPrompts) setSweep(true);
    revealSections(body.current);
  }, [reveal, hasPrompts]);

  const allDays = useMemo(() => (data ? buildDays(data, range) : []), [data, range]);
  const days = useMemo(
    () => allDays.map((d) => ({ ...d, total: agent ? (d.counts[agent] ?? 0) : Object.values(d.counts).reduce((a, b) => a + b, 0) })),
    [allDays, agent],
  );
  const names = useMemo(() => new Map((data?.agents ?? []).map((a) => [a.key, a.display_name])), [data]);
  const promptAgents = useMemo(() => {
    const n = new Map<string, number>();
    for (const a of data?.activity ?? []) n.set(a.agent, (n.get(a.agent) ?? 0) + a.prompts);
    return [...n.entries()].sort((a, b) => b[1] - a[1]).map(([k]) => k);
  }, [data]);

  const first = data?.activity[0]?.day;
  const fromLabel = first ? `${MON[Number(first.slice(5, 7)) - 1]} ${first.slice(0, 4)}` : "";
  const name = (k: string) => names.get(k) ?? k;
  const shown = moreChips ? promptAgents : promptAgents.slice(0, CHIPS_SHOWN);

  return (
    <section ref={body} className="flex w-full max-w-[1680px] flex-col gap-6">
      <PageHeader title={t.dashboard} />
      {error ? (
        <p role="alert" className="text-danger">
          {getErrorMessage(error, t.loadFailed)}
        </p>
      ) : null}
      {loading || !data ? null : !data.activity.length ? (
        <>
          {scanning ? (
            // First scan: only what is found so far; the rest rises in with the prompts.
            <FirstScan data={data} />
          ) : (
            <>
              <p className="m-0 rounded-card bg-panel px-5 py-4 text-[13.5px] text-ink-2">{t.noPromptsFound}</p>
              <Attention />
              <Skills agent={null} />
              <AgentTable data={data} days={[]} />
            </>
          )}
        </>
      ) : (
        <>
          <div role="toolbar" aria-label={t.filterDashboard} className="flex flex-wrap items-center gap-2">
            <Menu
              label={t.dateRange}
              value={range}
              onChange={setRange}
              icon={
                <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true" className="text-ink-2">
                  <rect x="2.5" y="3.5" width="11" height="10" rx="2" />
                  <path d="M2.5 6.5h11M5.5 2v3M10.5 2v3" strokeLinecap="round" />
                </svg>
              }
              options={[
                { value: 0, label: t.allTime, hint: fromLabel ? t.sinceMonth(fromLabel) : undefined },
                ...t.rangeLabel.map((l, i) => ({ value: i + 1, label: l })),
              ]}
            />
            <span className="mx-1 h-4 w-px bg-rule" />
            <Chip on={agent === null} onClick={() => setAgent(null)}>
              {t.allAgents}
            </Chip>
            {shown.map((k) => (
              <Chip key={k} on={agent === k} onClick={() => setAgent(agent === k ? null : k)}>
                <AgentLogo agent={k} size={14} />
                {name(k)}
              </Chip>
            ))}
            {promptAgents.length > CHIPS_SHOWN ? (
              <Chip on={false} onClick={() => setMoreChips(!moreChips)} expanded={moreChips}>
                {moreChips ? t.fewerAgents : t.moreAgents(promptAgents.length - CHIPS_SHOWN)}
              </Chip>
            ) : null}
            {agent !== null || range !== 0 ? (
              <button
                type="button"
                onClick={() => {
                  setAgent(null);
                  setRange(0);
                }}
                className="ml-auto h-[30px] rounded-full px-3 text-[12.5px] font-semibold text-ink-2 hover:bg-sunk"
              >
                {t.reset}
              </button>
            ) : null}
          </div>
          <Totals data={data} days={days} agent={agent} range={range} />
          <Calendar
            sweep={sweep}
            name={name}
            days={days}
            hours={range === 0 ? data.hours.filter((h) => !agent || h.agent === agent) : null}
            rangeText={t.rangeShort(fromLabel)[range] ?? ""}
          />
          <div className="grid grid-cols-[minmax(0,1.7fr)_minmax(0,1fr)] gap-5 max-[980px]:grid-cols-1">
            <PromptBars days={days} agent={agent} name={name} />
            <Attention />
          </div>
          <Skills agent={agent} />
          <AgentTable data={data} days={days} />
        </>
      )}
    </section>
  );
}

function Chip({ on, onClick, expanded, children }: { on: boolean; onClick: () => void; expanded?: boolean; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={expanded === undefined ? on : undefined}
      aria-expanded={expanded}
      onClick={onClick}
      className={[
        "inline-flex h-[30px] items-center gap-1.5 rounded-full px-3 text-[12.5px] font-medium transition-transform duration-100 active:scale-[0.97]",
        on ? "bg-sunk text-ink ring-[1.5px] ring-ink" : "bg-bg text-ink-2 ring-1 ring-rule hover:bg-panel",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

const card = "flex min-w-0 flex-col gap-4 rounded-card bg-bg p-5 ring-1 ring-rule";
const head = "flex min-h-8 flex-wrap items-center gap-3";
const h2 = "m-0 text-[15px] font-bold tracking-[-0.01em]";
const more = "ml-auto text-[12.5px] font-semibold text-ink-3 hover:text-ink";

function Totals({ data, days, agent, range }: { data: Overview; days: Day[]; agent: string | null; range: number }) {
  const total = days.reduce((a, d) => a + d.total, 0);
  const active = days.filter((d) => d.total > 0).length;
  const quiet = days.length - active;
  let streak = 0;
  for (let i = days.length - 1; i >= 0; i--) {
    if (days[i]!.total > 0) streak++;
    else if (i === days.length - 1)
      continue; // today may still be empty
    else break;
  }
  const row = agent ? data.agent_rows.find((r) => r.agent === agent) : null;
  let longest = 0;
  let run = 0;
  for (const d of days) {
    run = d.total > 0 ? run + 1 : 0;
    longest = Math.max(longest, run);
  }
  const navigate = useNavigate();
  const toAgents = () => document.getElementById("agents")?.scrollIntoView({ behavior: reducedMotion() ? "auto" : "smooth", block: "start" });
  const agentKeys = data.agent_rows.map((r) => r.agent);
  const items: StatItem[] = [
    {
      label: t.prompts,
      value: formatCount(total),
      hint: active ? t.perActiveDay(Math.round(total / active)) : t.noPromptsYetCell,
      onClick: () => navigate(withParams("/sessions", { agent: row?.agent })),
    },
    {
      label: t.activeDaysT,
      value: formatCount(active),
      meter: days.length ? active / days.length : 0,
      hint: t.ofDays(formatCount(days.length), formatCount(quiet)),
    },
    { label: t.currentStreak, value: t.daysN(streak), hint: t.longestN(longest) },
  ];
  if (range === 0) {
    items.push({
      label: row ? t.skillsT : t.skillsUsedT,
      value: formatCount(row ? row.skills : (data.skills_used ?? 0)),
      hint: t.skillsHint,
      // An agent picked: its skills; all agents: the skills that ran.
      onClick: () => navigate(row ? withParams("/skills", { agent: row.agent }) : "/skills?status=used"),
    });
    items.push(
      row
        ? { label: t.sessionsLabel, value: formatCount(row.sessions), onClick: () => navigate(withParams("/sessions", { agent: row.agent })) }
        : {
            label: t.agentsT,
            value: formatCount(agentKeys.length),
            aside: <AgentStack agents={agentKeys} name={(k) => data.agent_rows.find((r) => r.agent === k)?.name ?? k} size={26} card={false} />,
            hint: t.seeAgents,
            onClick: toAgents,
          },
    );
  }
  return <StatTiles label={t.prompts} items={items} min={190} />;
}

function Calendar({
  days,
  hours,
  rangeText,
  name,
  sweep = false,
}: {
  days: Day[];
  hours: Overview["hours"] | null;
  rangeText: string;
  name: (k: string) => string;
  sweep?: boolean;
}) {
  // Hovered or keyboard-focused day (index into `days`).
  const [tip, setTip] = useState<number | null>(null);
  const [focused, setFocused] = useState(false);
  const gridRef = useRef<HTMLDivElement>(null);
  const padN = days[0]?.dow ?? 0;
  const swept = useRef(false);
  useEffect(() => {
    if (!sweep || swept.current || !days.length) return;
    swept.current = true;
    sweepGrid(gridRef.current);
  }, [sweep, days.length]);
  if (!days.length) return null;
  const weeks = Math.ceil((days.length + padN) / 7);
  // Cells grow with the card (capped, so a 30-day range is not huge).
  const cell = 36;
  let bi = 0;
  days.forEach((d, i) => {
    if (d.total > days[bi]!.total) bi = i;
  });
  let run = 0;
  let longest = 0;
  for (const d of days) {
    run = d.total > 0 ? run + 1 : 0;
    longest = Math.max(longest, run);
  }
  const months: { col: number; label: string }[] = [];
  days.forEach((d, j) => {
    const col = Math.floor((j + padN) / 7) + 1;
    const m = d.date.getMonth();
    if (d.date.getDate() === 1) months.push({ col, label: m === 0 ? String(d.date.getFullYear()) : MON[m]! });
    else if (j === 0 && d.date.getDate() <= 17) months.push({ col, label: MON[m]! });
  });
  const busy = days[bi]!;
  let peak: string | null = null;
  let late: string | null = null;
  if (hours?.length) {
    const grid = new Map<string, number>();
    let all = 0;
    let after9 = 0;
    for (const h of hours) {
      grid.set(`${h.dow}-${h.hour}`, (grid.get(`${h.dow}-${h.hour}`) ?? 0) + h.prompts);
      all += h.prompts;
      if (h.hour >= 21) after9 += h.prompts;
    }
    const k = [...grid.entries()].sort((a, b) => b[1] - a[1])[0]![0];
    const [dw = 0, hr = 0] = k.split("-").map(Number);
    peak = `${DOW[dw]} ${hourName(hr)}`;
    late = `${Math.round((after9 / Math.max(all, 1)) * 100)}%`;
  }
  return (
    <section aria-labelledby="cal-h" className={card}>
      <div className={head}>
        <h2 id="cal-h" className={h2}>
          {t.activity}
        </h2>
        <span className="ml-auto text-[12.5px] text-ink-3">{rangeText}</span>
      </div>
      <div className="w-full" style={{ maxWidth: weeks * cell }}>
        <div className="mb-1 grid h-4 text-[11px] text-ink-3" style={{ gridTemplateColumns: `repeat(${weeks},minmax(0,1fr))` }}>
          {months.map((m) => (
            <span key={`${m.col}-${m.label}`} className="whitespace-nowrap" style={{ gridColumn: `${m.col} / span 4`, gridRow: 1 }}>
              {m.label}
            </span>
          ))}
        </div>
        <div className="relative">
          <div
            ref={gridRef}
            className="grid grid-flow-col gap-[2px] rounded-[4px] focus-visible:outline-none"
            style={{ gridTemplateRows: "repeat(7,auto)", gridAutoColumns: "minmax(0,1fr)" }}
            role="application"
            tabIndex={0}
            aria-roledescription={t.heatmapRole}
            aria-label={`${t.activity}: ${days.filter((d) => d.total > 0).length} ${t.activeDaysL}. ${t.heatmapKeys}`}
            onMouseMove={(e) => {
              // Gaps between cells keep the day under the pointer, so the card does not blink.
              const i = (e.target as HTMLElement).dataset.i;
              if (i !== undefined && Number(i) !== tip) setTip(Number(i));
            }}
            onMouseLeave={() => !focused && setTip(null)}
            onFocus={() => {
              setFocused(true);
              setTip((v) => v ?? days.length - 1);
            }}
            onBlur={() => {
              setFocused(false);
              setTip(null);
            }}
            onKeyDown={(e) => {
              const step = { ArrowLeft: -7, ArrowRight: 7, ArrowUp: -1, ArrowDown: 1, Home: -Infinity, End: Infinity }[e.key];
              if (step === undefined) return;
              e.preventDefault();
              setTip((v) => {
                const cur = v ?? days.length - 1;
                return Math.max(0, Math.min(days.length - 1, Number.isFinite(step) ? cur + step : step < 0 ? 0 : days.length - 1));
              });
            }}
          >
            {Array.from({ length: padN }, (_, i) => (
              <i key={`p${i}`} className="invisible block aspect-square" />
            ))}
            {days.map((d, i) => (
              <i
                key={d.key}
                data-i={i}
                className="block aspect-square rounded-[2px]"
                style={{
                  background: `var(--heat-${level(d.total)})`,
                  boxShadow: i === tip ? "0 0 0 2px var(--accent)" : i === bi && d.total > 0 ? "0 0 0 2px var(--ink)" : undefined,
                  position: i === tip ? "relative" : undefined,
                  zIndex: i === tip ? 1 : undefined,
                }}
              />
            ))}
          </div>
          {tip !== null && days[tip] ? <DayTip day={days[tip]!} index={tip + padN} weeks={weeks} name={name} /> : null}
          <span className="sr-only" aria-live="polite">
            {focused && tip !== null && days[tip] ? `${longDay(days[tip]!.date)}, ${formatCount(days[tip]!.total)} ${t.promptsWord(days[tip]!.total)}` : ""}
          </span>
        </div>
      </div>
      <div className="flex flex-wrap gap-x-[18px] gap-y-2 text-[12.5px] text-ink-3">
        <span>
          {t.busiestDay} <b className="tnum font-semibold text-ink">{busy.total > 0 ? `${longDay(busy.date)}, ${formatCount(busy.total)}` : "—"}</b>
        </span>
        <span>
          {t.longestStreak}{" "}
          <b className="tnum font-semibold text-ink">
            {t.daysN(longest)}
            {longest > 0 && run === longest ? `, ${t.now}` : ""}
          </b>
        </span>
        {peak ? (
          <>
            <span>
              {t.peakHour} <b className="font-semibold text-ink">{peak}</b>
            </span>
            <span>
              {t.after9} <b className="tnum font-semibold text-ink">{late}</b>
            </span>
          </>
        ) : null}
        <span className="ml-auto flex items-center gap-1.5">
          {t.fewer}
          <span className="inline-flex gap-[2px]">
            {[0, 1, 2, 3, 4, 5].map((l) => (
              <i key={l} className="block h-2.5 w-2.5 rounded-[2px]" style={{ background: `var(--heat-${l})` }} />
            ))}
          </span>
          {t.more}
        </span>
      </div>
    </section>
  );
}

/** Hover / focus card for one heatmap day: date, total, prompts per agent. */
function DayTip({ day, index, weeks, name }: { day: Day; index: number; weeks: number; name: (k: string) => string }) {
  const col = Math.floor(index / 7);
  const row = index % 7;
  const x = ((col + 0.5) / weeks) * 100;
  const right = x > 70;
  const parts = Object.entries(day.counts)
    .filter(([, n]) => n > 0)
    .sort((a, b) => b[1] - a[1]);
  return (
    <div
      role="tooltip"
      className="pointer-events-none absolute z-10 min-w-[180px] rounded-row bg-bg px-3 py-2.5 text-[12.5px] shadow-card ring-1 ring-rule"
      style={{
        left: `${x}%`,
        top: `calc(${((row + 1) / 7) * 100}% + 10px)`,
        transform: right ? "translateX(calc(-100% + 12px))" : "translateX(-12px)",
      }}
    >
      <div className="font-semibold text-ink">{longDay(day.date)}</div>
      <div className="tnum mt-0.5 text-ink-2">
        {formatCount(day.total)} {t.promptsWord(day.total)}
      </div>
      {parts.length ? (
        <ul className="m-0 mt-2 flex list-none flex-col gap-1 border-t border-rule-2 p-0 pt-2">
          {parts.map(([k, n]) => (
            <li key={k} className="grid grid-cols-[16px_1fr_auto] items-center gap-2 text-ink-2">
              <AgentLogo agent={k} size={14} />
              <span className="truncate">{name(k)}</span>
              <span className="tnum font-semibold text-ink">{formatCount(n)}</span>
            </li>
          ))}
        </ul>
      ) : (
        <div className="mt-1 text-ink-3">{t.quietDay}</div>
      )}
    </div>
  );
}

type Bin = { label: string; full: string; v: Record<string, number>; tot: number };

function niceStep(max: number) {
  if (max <= 0) return 1;
  const raw = max / 4;
  const mag = 10 ** Math.floor(Math.log10(raw));
  return Math.max(1, [1, 2, 5, 10].map((q) => q * mag).find((q) => q >= raw) ?? mag * 10);
}

function PromptBars({ days, agent, name }: { days: Day[]; agent: string | null; name: (k: string) => string }) {
  const [share, setShare] = useState(false);
  const [hover, setHover] = useState<number | null>(null);
  const monthly = days.length > 120;
  const bins: Bin[] = [];
  let lastKey = "";
  let lastYear = -1;
  days.forEach((d, j) => {
    const key = monthly ? `${d.date.getFullYear()}-${d.date.getMonth()}` : String(Math.floor((j + days[0]!.dow) / 7));
    if (key !== lastKey) {
      lastKey = key;
      const m = d.date.getMonth();
      const y = d.date.getFullYear();
      let label = monthly ? MON[m]! : `${d.date.getDate()} ${MON[m]}`;
      if (monthly && y !== lastYear) label += ` ’${String(y).slice(2)}`;
      lastYear = y;
      bins.push({ label, full: monthly ? `${MON[m]} ${y}` : t.weekOf(`${d.date.getDate()} ${MON[m]} ${y}`), v: {}, tot: 0 });
    }
    const b = bins[bins.length - 1]!;
    for (const [k, n] of Object.entries(d.counts)) {
      if (agent && k !== agent) continue;
      b.v[k] = (b.v[k] ?? 0) + n;
      b.tot += n;
    }
  });
  const order = [...new Set(bins.flatMap((b) => Object.keys(b.v)))].sort(
    (a, b) => bins.reduce((s, x) => s + (x.v[b] ?? 0), 0) - bins.reduce((s, x) => s + (x.v[a] ?? 0), 0),
  );
  const pct = share && !agent;
  const step = niceStep(Math.max(0, ...bins.map((b) => b.tot)));
  const top = pct ? 100 : step * 4;
  const every = monthly ? (bins.length <= 12 ? 1 : 3) : bins.length <= 8 ? 1 : 2;
  const empty = bins.every((b) => b.tot === 0);
  return (
    <section aria-labelledby="shift-h" className={card}>
      <div className={head}>
        <h2 id="shift-h" className={h2}>
          {t.promptsByAgent}
        </h2>
        {!agent ? (
          <div role="tablist" aria-label={t.measure} className="ml-auto inline-flex gap-0.5 rounded-full bg-sunk p-[3px]">
            {[false, true].map((v) => (
              <button
                key={String(v)}
                type="button"
                role="tab"
                aria-selected={share === v}
                onClick={() => setShare(v)}
                className={[
                  "h-7 rounded-full px-3 text-[12.5px]",
                  share === v ? "bg-bg font-semibold text-ink shadow-sm ring-1 ring-rule" : "font-medium text-ink-2",
                ].join(" ")}
              >
                {v ? t.share : t.prompts}
              </button>
            ))}
          </div>
        ) : null}
      </div>
      {empty ? (
        <div className="grid h-[220px] place-items-center rounded-[12px] border border-dashed border-rule text-[12.5px] text-ink-3">
          {agent ? t.noPromptsFrom(name(agent)) : t.noPromptsYet}
        </div>
      ) : (
        <>
          <div className="grid grid-cols-[44px_minmax(0,1fr)] grid-rows-[220px_auto] gap-x-2.5 gap-y-2">
            <div className="relative" aria-hidden="true">
              {[0, 1, 2, 3, 4].map((i) => (
                <span key={i} className="tnum absolute right-0 translate-y-1/2 whitespace-nowrap text-[11.5px] text-ink-3" style={{ bottom: `${i * 25}%` }}>
                  {pct ? `${i * 25}%` : formatCount(i * step)}
                </span>
              ))}
            </div>
            <div className="relative">
              {[0, 1, 2, 3, 4].map((i) => (
                <i
                  key={i}
                  className={["absolute inset-x-0 block border-t", i === 0 ? "border-rule" : "border-rule-2"].join(" ")}
                  style={{ bottom: `${i * 25}%` }}
                />
              ))}
              <div
                className="absolute inset-0 flex gap-1.5"
                role="list"
                aria-label={t.promptsByAgent}
                // The bar under the pointer comes from its x position, so the 6px
                // gaps between bars never clear the card (no blink between bars).
                onPointerMove={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  const i = Math.max(0, Math.min(bins.length - 1, Math.floor(((e.clientX - r.left) / r.width) * bins.length)));
                  if (i !== hover) setHover(i);
                }}
                onPointerLeave={() => setHover(null)}
              >
                {bins.map((b, i) => {
                  const rows = order.filter((k) => b.v[k]);
                  const aria = `${b.full}: ${rows.length ? rows.map((k) => `${name(k)} ${formatCount(b.v[k] ?? 0)}`).join(", ") : t.noPromptsYet}`;
                  return (
                    <div
                      key={b.full}
                      role="listitem"
                      tabIndex={0}
                      aria-label={aria}
                      onFocus={() => setHover(i)}
                      onBlur={() => setHover(null)}
                      className={[
                        "relative flex min-w-0 flex-1 items-end rounded-md outline-none focus-visible:ring-2 focus-visible:ring-accent",
                        hover === i ? "bg-sunk/60" : "",
                      ].join(" ")}
                    >
                      <div className="flex h-full w-full flex-col-reverse gap-[1.5px]">
                        {rows.map((k, ri) => (
                          <i
                            key={k}
                            className={["block w-full flex-none", ri === rows.length - 1 ? "rounded-t-[4px]" : ""].join(" ")}
                            style={{ height: `${(((b.v[k] ?? 0) / (pct ? b.tot : top)) * 100).toFixed(2)}%`, background: agentColor(k) }}
                          />
                        ))}
                      </div>
                    </div>
                  );
                })}
              </div>
              {hover !== null && bins[hover] ? <BarTip bin={bins[hover]!} index={hover} count={bins.length} order={order} pct={pct} name={name} /> : null}
            </div>
            <span />
            <div className="flex gap-1.5" aria-hidden="true">
              {bins.map((b, i) => (
                <span key={b.full} className="min-w-0 flex-1 whitespace-nowrap text-center text-[11.5px] text-ink-3">
                  {i % every === 0 ? b.label : ""}
                </span>
              ))}
            </div>
          </div>
          <div className="flex flex-wrap justify-center gap-3.5 text-[12.5px] text-ink-2">
            {order.map((k) => (
              <span key={k} className="inline-flex items-center gap-1.5">
                <i className="h-2.5 w-2.5 rounded-[3px]" style={{ background: agentColor(k) }} />
                {name(k)}
              </span>
            ))}
          </div>
        </>
      )}
    </section>
  );
}

/** The one hover card for the bar chart. It glides to the bar under the pointer and flips to the bar's left side in the right half. */
function BarTip({ bin, index, count, order, pct, name }: { bin: Bin; index: number; count: number; order: string[]; pct: boolean; name: (k: string) => string }) {
  const rows = order.filter((k) => bin.v[k]);
  const flip = index > count * 0.55;
  const left = flip ? `calc(${(index / count) * 100}% - 6px)` : `calc(${((index + 1) / count) * 100}% + 6px)`;
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute top-2 z-10 flex min-w-[168px] flex-col gap-1 whitespace-nowrap rounded-[10px] bg-bg px-2.5 py-2 text-[12px] shadow-card ring-1 ring-rule transition-[left,transform] duration-150 ease-out"
      style={{ left, transform: flip ? "translateX(-100%)" : "none" }}
    >
      <span className="font-semibold text-ink">{bin.full}</span>
      {rows.length ? (
        rows.map((k) => (
          <span key={k} className="grid grid-cols-[8px_1fr_auto] items-center gap-2 text-ink-2">
            <i className="h-2 w-2 rounded-[2px]" style={{ background: agentColor(k) }} />
            {name(k)}
            <b className="tnum font-mono font-normal text-ink">{pct ? `${Math.round(((bin.v[k] ?? 0) / bin.tot) * 100)}%` : formatCount(bin.v[k] ?? 0)}</b>
          </span>
        ))
      ) : (
        <span className="text-ink-3">{t.noPromptsYet}</span>
      )}
      {rows.length > 1 ? (
        <span className="grid grid-cols-[8px_1fr_auto] items-center gap-2 border-t border-rule-2 pt-1 text-ink">
          <i />
          {t.total}
          <b className="tnum font-mono font-normal">{formatCount(bin.tot)}</b>
        </span>
      ) : null}
    </div>
  );
}

function Attention() {
  const fixes = useFixes();
  const navigate = useNavigate();
  const list: Fix[] = fixes.data ?? [];
  return (
    <section aria-labelledby="fx-h" className="flex min-w-0 flex-col gap-0.5 rounded-card bg-panel p-5">
      <div className={head}>
        <h2 id="fx-h" className={h2}>
          {t.needsAttention}
        </h2>
        {list.length ? (
          <Link to="/fixes" className={more}>
            {t.allN(list.length)}
          </Link>
        ) : null}
      </div>
      {fixes.isSuccess && !list.length ? <p className="m-0 mt-3 text-[12.5px] text-ink-3">{t.nothingNeeds}</p> : null}
      <ul className="m-0 list-none p-0">
        {list.slice(0, 3).map((f) => {
          const u = urgency(f.rule);
          return (
            <li key={f.id} className="border-b border-rule-2 last:border-b-0">
              <button
                type="button"
                onClick={() => navigate(f.link || "/fixes")}
                className="grid w-full grid-cols-[auto_1fr] items-center gap-x-2.5 gap-y-1 py-3 text-left"
              >
                <span className={["rounded-full px-2 py-0.5 text-[11.5px] font-medium", u.tone].join(" ")}>{u.label}</span>
                <span className="truncate text-[13.5px] font-semibold tracking-[-0.01em]">{f.title}</span>
                <span className="col-start-2 line-clamp-2 text-[12.5px] text-ink-3">{f.why}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function Skills({ agent }: { agent: string | null }) {
  const q = useItems("skill");
  const navigate = useNavigate();
  const all = (q.data ?? []).filter((s: Item) => !agent || s.agents.includes(agent));
  const ran = all.filter((s) => s.status.state !== "gone" && (s.usage?.count ?? 0) > 0).sort((a, b) => (b.usage?.count ?? 0) - (a.usage?.count ?? 0));
  const never = all.filter((s) => s.status.state === "unused").sort((a, b) => a.name.localeCompare(b.name));
  const max = ran[0]?.usage?.count ?? 1;
  const desc = (s: Item) => (s.facts.kind === "skill" ? (s.facts.description ?? "") : "");
  return (
    <div className="grid grid-cols-2 gap-5 max-[980px]:grid-cols-1">
      <section aria-labelledby="sk-h" className={card}>
        <div className={head}>
          <h2 id="sk-h" className={h2}>
            {t.topSkills}
          </h2>
          <Link to={withParams("/skills", { agent })} className={more}>
            {t.allN(all.filter((s) => s.status.state !== "gone" && s.status.state !== "not_an_item").length)}
          </Link>
        </div>
        {q.isSuccess && !ran.length ? <p className="m-0 text-[12.5px] text-ink-3">{t.noSkillRuns}</p> : null}
        <div className="flex flex-col gap-1.5">
          {ran.slice(0, LIST_ROWS).map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => navigate(itemRoute(s))}
              title={t.openItem(s.name)}
              className="group grid min-h-9 w-full grid-cols-[minmax(0,1fr)_72px] items-center gap-3 rounded-[10px] text-left transition-transform duration-100 active:scale-[0.985]"
            >
              <span className="relative block h-9 overflow-hidden rounded-[10px] bg-panel transition-colors duration-100 group-hover:bg-sunk">
                <i
                  className="absolute inset-y-0 left-0 min-w-2 rounded-[10px]"
                  style={{ width: `${((s.usage?.count ?? 0) / max) * 100}%`, background: "var(--heat-1)" }}
                />
                <span className="absolute inset-0 flex items-center truncate px-3 text-[13px] font-semibold">{s.name}</span>
              </span>
              <span className="tnum text-right text-[13.5px] font-semibold leading-tight">
                {formatCount(s.usage?.count ?? 0)}
                <small className="block text-[11.5px] font-normal text-ink-3">{t.runsL}</small>
              </span>
            </button>
          ))}
        </div>
      </section>
      <section aria-labelledby="nr-h" className={card}>
        <div className={head}>
          <h2 id="nr-h" className={h2}>
            {t.neverRan}
          </h2>
          {never.length ? (
            <Link to={withParams("/skills", { status: "unused", agent })} className={more}>
              {t.skillsN(never.length)}
            </Link>
          ) : null}
        </div>
        {q.isSuccess && !never.length ? <p className="m-0 text-[12.5px] text-ink-3">{t.noneNeverRan}</p> : null}
        <ul className="m-0 list-none p-0">
          {never.slice(0, LIST_ROWS).map((s) => (
            <li key={s.id} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-0.5 border-b border-rule-2 py-2.5 last:border-b-0">
              <span className="truncate text-[13.5px] font-semibold">{s.name}</span>
              <button
                type="button"
                onClick={() => navigate(itemRoute(s))}
                aria-label={`${t.review} ${s.name}`}
                className="col-start-2 row-span-2 row-start-1 h-8 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
              >
                {t.review}
              </button>
              <span className="truncate text-[12.5px] text-ink-3">{desc(s)}</span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}

function AgentTable({ data, days }: { data: Overview; days: Day[] }) {
  const prompts = new Map<string, number>();
  let total = 0;
  for (const d of days)
    for (const [k, n] of Object.entries(d.counts)) {
      prompts.set(k, (prompts.get(k) ?? 0) + n);
      total += n;
    }
  // Every agent in one list: deep agents first (by prompts), then skills-only agents (by skills).
  const base = [...data.agent_rows].sort(
    (a, b) =>
      Number(b.tier === "full") - Number(a.tier === "full") ||
      (prompts.get(b.agent) ?? 0) - (prompts.get(a.agent) ?? 0) ||
      b.skills - a.skills ||
      a.name.localeCompare(b.name),
  );
  const { sort, toggle: onSort } = useSort<"agent" | "share" | "skills" | "hooks" | "mcp" | "sessions">();
  const rows = sortBy(base, sort, (r, k) =>
    k === "agent"
      ? r.name
      : k === "share"
        ? (prompts.get(r.agent) ?? 0)
        : k === "skills"
          ? r.skills
          : r.tier !== "full"
            ? null
            : k === "hooks"
              ? r.hooks
              : k === "mcp"
                ? r.mcp
                : r.sessions,
  );
  const muted = "text-ink-3";
  const deep = data.agent_rows.filter((r) => r.tier === "full").length;
  return (
    <section id="agents" aria-labelledby="ag-h" className={card}>
      <div className={head}>
        <h2 id="ag-h" className={h2}>
          {t.agentsAtGlance}
        </h2>
        <span className="ml-auto text-[12.5px] text-ink-3">
          {t.deepN(deep)} · {t.skillsOnlyN(data.agent_rows.length - deep)}
        </span>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[720px] border-collapse text-left text-[14px]">
          <thead>
            <tr className="text-[12.5px] text-ink-3">
              <Th table="dash-agents" label={t.colAgent} k="agent" first="asc" sort={sort} onSort={onSort} className="pr-4" />
              <Th table="dash-agents" label={t.colShare} k="share" sort={sort} onSort={onSort} className="w-[26%] pr-4" />
              <Th table="dash-agents" label={t.colSkills} k="skills" sort={sort} onSort={onSort} align="right" className="w-20 pr-4" />
              <Th table="dash-agents" label={t.colHooks} k="hooks" sort={sort} onSort={onSort} align="right" className="w-20 pr-4" />
              <Th table="dash-agents" label={t.colMcp} k="mcp" sort={sort} onSort={onSort} align="right" className="w-20 pr-4" />
              <Th table="dash-agents" label={t.colSessions} k="sessions" sort={sort} onSort={onSort} align="right" resize={false} className="w-24" />
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              const p = prompts.get(r.agent) ?? 0;
              const s = total ? p / total : 0;
              const isDeep = r.tier === "full";
              return (
                <tr key={r.agent} className="h-11 border-t border-rule-2 hover:bg-sunk/50">
                  <td className="pr-4">
                    <span className="flex items-center gap-2 font-semibold">
                      <AgentLogo agent={r.agent} size={18} />
                      <span className="truncate">{r.name}</span>
                      {isDeep ? null : (
                        <span className="shrink-0 rounded-full bg-sunk px-2 text-[11.5px] font-medium leading-5 text-ink-3">{t.skillsOnlyCell}</span>
                      )}
                    </span>
                  </td>
                  <td className="pr-4">
                    {!isDeep ? (
                      <span className={muted}>—</span>
                    ) : p === 0 ? (
                      <span className={muted}>{t.noPromptsYetCell}</span>
                    ) : s < 0.01 ? (
                      <span className={muted}>{t.lessThan1}</span>
                    ) : (
                      <span className="flex items-center gap-2">
                        <span className="block h-1.5 min-w-20 flex-1 overflow-hidden rounded-full bg-sunk">
                          <i className="block h-full rounded-full bg-accent" style={{ width: `${s * 100}%` }} />
                        </span>
                        <span className="tnum w-9 text-right text-[12.5px] text-ink-2">{Math.round(s * 100)}%</span>
                      </span>
                    )}
                  </td>
                  {/* A count opens that agent's list. */}
                  {[
                    { n: r.skills, to: "/skills" },
                    ...(isDeep
                      ? [
                          { n: r.hooks, to: "/hooks" },
                          { n: r.mcp, to: "/mcp" },
                          { n: r.sessions, to: "/sessions" },
                        ]
                      : [null, null, null]),
                  ].map((c, i) => (
                    <td key={i} className={["tnum pr-4 text-right last:pr-0", !c || c.n === 0 ? muted : ""].join(" ")}>
                      {!c ? (
                        "—"
                      ) : c.to && c.n > 0 ? (
                        <Link
                          to={withParams(c.to, { agent: r.agent })}
                          className="rounded-[4px] underline decoration-transparent underline-offset-[3px] transition-colors duration-100 hover:text-accent-text hover:decoration-current"
                        >
                          {formatCount(c.n)}
                        </Link>
                      ) : (
                        formatCount(c.n)
                      )}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

const STAGES: { key: ScanStage; label: string }[] = [
  { key: "config", label: t.stageConfig },
  { key: "transcripts", label: t.stageSessions },
  { key: "deep", label: t.stageDeep },
];
const FOUND: { kind: ItemKind; to: string }[] = [
  { kind: "skill", to: "/skills" },
  { kind: "plugin", to: "/plugins" },
  { kind: "hook", to: "/hooks" },
  { kind: "mcp_server", to: "/mcp" },
  { kind: "subagent", to: "/subagents" },
  { kind: "memory", to: "/memory" },
];

/**
 * First scan, before any prompt is read: what is found so far, counting up in
 * place as each stage lands, over a stage list with a running bar. Gives way
 * to the full Dashboard when the prompts arrive.
 */
function FirstScan({ data }: { data: Overview }) {
  const stages = useScanStages();
  const scanning = useScanning();
  const running = scanning ? STAGES.find((s) => !stages.includes(s.key))?.key : undefined;
  const count = (k: ItemKind) => data.kinds.find((c) => c.kind === k)?.count ?? 0;
  const agents = data.agents.filter((a) => a.tier !== "shared");
  const items: StatItem[] = [
    {
      label: t.agentsT,
      value: formatCount(agents.length),
      aside: agents.length ? <AgentStack agents={agents.map((a) => a.key)} name={(k) => agents.find((a) => a.key === k)?.display_name ?? k} size={26} card={false} /> : undefined,
    },
    ...FOUND.map((f) => ({ label: kindLabel[f.kind], value: formatCount(count(f.kind)), to: f.to })),
  ];
  return (
    <section aria-labelledby="first-h" aria-busy={scanning} className="flex flex-col gap-4">
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-2">
        <h2 id="first-h" className="m-0 text-[20px] font-bold tracking-[-0.03em]">
          {scanning ? t.readingThisMac : t.foundOnThisMac}
        </h2>
        <ol className="m-0 flex list-none flex-wrap items-center gap-x-4 gap-y-1 p-0 text-[12.5px]">
          {STAGES.map((s) => {
            const done = stages.includes(s.key);
            const now = s.key === running;
            return (
              <li key={s.key} className={["inline-flex items-center gap-1.5 transition-colors duration-200", done ? "text-ink-2" : now ? "text-ink" : "text-ink-3"].join(" ")}>
                {done ? (
                  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" className="stage-check text-ok">
                    <path d="M2.5 6.2 5 8.5l4.5-5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                ) : (
                  <i className={["h-1.5 w-1.5 rounded-full", now ? "animate-pulse bg-accent" : "bg-rule"].join(" ")} aria-hidden="true" />
                )}
                {s.label}
                <span className="sr-only">{done ? t.stageDone : now ? t.stageRunning : t.stageQueued}</span>
              </li>
            );
          })}
        </ol>
      </div>
      <div className={["scan-bar h-1 overflow-hidden rounded-full bg-rule-2", scanning ? "" : "opacity-0"].join(" ")} aria-hidden="true">
        <span className="block h-full w-full rounded-full" />
      </div>
      <StatTiles label={t.foundOnThisMac} items={items} min={150} />
      <p className="m-0 text-[13px] text-ink-3">{scanning ? t.readingSessions : t.noPromptsFound}</p>
    </section>
  );
}

function reducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}
