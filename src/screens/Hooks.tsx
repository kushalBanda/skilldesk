import { StatTiles } from "../components/StatTiles";
import { PageHeader } from "../components/PageHeader";
import { AgentLogo } from "../components/AgentLogo";
import { SearchField } from "../components/SearchField";
import { Th, sortBy, useSort } from "../components/SortHeader";
import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useItems, useOverview } from "../ipc/queries";
import { useAction } from "../actions/ActionProvider";
import { getErrorMessage } from "../ipc/error";
import type { Item } from "../ipc/bindings/Item";
import type { HookFacts } from "../ipc/bindings/HookFacts";
import type { AgentSummary } from "../ipc/bindings/AgentSummary";
import { FChip } from "../components/FChip";
import { AgentFilter } from "../components/AgentFilter";
import { GroupToggle } from "../components/GroupToggle";
import { DetailSheet, SheetSteps } from "../components/DetailSheet";
import { formatCount, formatMs, tildePath } from "../lib/format";
import { PathLink, PluginName } from "../components/PlaceLink";
import { t } from "../strings";

type StatusFilter = "all" | "broken" | "slow" | "off" | "gone";
type GroupBy = "agent" | "owner";

const SLOW_P95_MS = 5_000;

function facts(item: Item): HookFacts | null {
  return item.facts.kind === "hook" ? item.facts : null;
}

export function Hooks() {
  const { data, error, isPending } = useItems("hook");
  const overview = useOverview();
  const agents = overview.data?.agents ?? [];
  const totals = overview.data?.hooks ?? null;
  const [params] = useSearchParams();
  const [status, setStatus] = useState<StatusFilter>((params.get("status") as StatusFilter) ?? "all");
  const [agent, setAgent] = useState<string | null>(params.get("agent"));
  const [group, setGroup] = useState<GroupBy>("agent");
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(params.get("id"));
  // Deep links (Fix list "Show me", tray) may arrive while this screen is open.
  useEffect(() => {
    setStatus((params.get("status") as StatusFilter) ?? "all");
    setAgent(params.get("agent"));
    setSelectedId(params.get("id"));
  }, [params]);

  const hooks = data ?? [];
  const selected = hooks.find((h) => h.id === selectedId) ?? null;
  const live = hooks.filter((h) => h.status.state !== "gone");
  const count = (st: string) => hooks.filter((h) => h.status.state === st).length;
  const agentName = (key: string) => agents.find((a) => a.key === key)?.display_name ?? key;

  const { sort, toggle: onSort } = useSort<"name" | "event" | "who" | "runs" | "p95">();
  const filtered = hooks.filter((h) => {
    const st = h.status.state;
    if (status === "all" ? st === "gone" : st !== status) return false;
    if (agent && !h.agents.includes(agent)) return false;
    const f = facts(h);
    if (query && !`${h.name} ${f?.event ?? ""} ${f?.owner ?? ""} ${f?.command ?? ""}`.toLowerCase().includes(query.toLowerCase())) return false;
    return true;
  });
  // Sorting orders rows inside each group; groups keep their order.
  const rows = sortBy(filtered, sort, (h, k) => {
    const f = facts(h);
    if (k === "name") return h.name;
    if (k === "event") return f?.event ?? null;
    if (k === "who") return (group === "agent" ? f?.owner : agentName(h.agents[0] ?? "")) || null;
    if (k === "runs") return h.usage ? Number(h.usage.count) : null;
    return h.usage?.timing?.p95_ms ?? null;
  });

  const groups = useMemo(() => {
    const m = new Map<string, Item[]>();
    for (const h of rows) {
      const key = group === "agent" ? (h.agents[0] ?? "") : (facts(h)?.owner ?? "");
      m.set(key, [...(m.get(key) ?? []), h]);
    }
    return [...m.entries()]
      .sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]))
      .map(([key, list]) => [key, cluster(list)] as const);
  }, [rows, group]);
  const [openClusters, setOpenClusters] = useState<Set<string>>(() => new Set());
  const [closedGroups, setClosedGroups] = useState<Set<string>>(() => new Set());
  const flip = (set: (f: (s: Set<string>) => Set<string>) => void, k: string) =>
    set((s) => {
      const next = new Set(s);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });

  const bars = useMemo(
    () =>
      hooks
        .filter((h) => (h.usage?.timing?.total_ms ?? 0) > 0)
        .sort((a, b) => (b.usage!.timing!.total_ms ?? 0) - (a.usage!.timing!.total_ms ?? 0))
        .slice(0, 6),
    [hooks],
  );
  const maxMs = bars[0]?.usage?.timing?.total_ms ?? 1;

  const chip = (k: StatusFilter, label: string, n: number) => (
    <FChip label={label} count={n} active={status === k} onClick={() => setStatus(status === k && k !== "all" ? "all" : k)} />
  );

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-6 overflow-auto page">
        <PageHeader
          title={t.hooks}
          search={<SearchField value={query} onChange={setQuery} label={t.searchHooks} count={query ? t.nResults(rows.length) : null} className="w-64" />}
        />
        {error ? (
          <p role="alert" className="text-danger">
            {getErrorMessage(error, t.loadFailed)}
          </p>
        ) : null}
        <StatTiles
          label={t.hooks}
          items={[
            ...(totals
              ? [
                  {
                    label: t.hoursInHooks,
                    value: formatMs(Number(totals.total_ms)),
                    hint: t.acrossRuns(formatCount(Number(totals.runs))),
                  },
                  {
                    label: t.hookRuns,
                    value: formatCount(Number(totals.runs)),
                    split: [
                      {
                        label: t.completed,
                        value: Math.max(0, Number(totals.runs) - Number(totals.cancelled) - Number(totals.failed) - Number(totals.blocked)),
                        tone: "ok" as const,
                      },
                      { label: t.cancelled, value: Number(totals.cancelled), tone: "warn" as const },
                      { label: t.failed, value: Number(totals.failed), tone: "danger" as const },
                      { label: t.blocked, value: Number(totals.blocked), tone: "muted" as const },
                    ],
                  },
                  {
                    label: t.cancelled,
                    value: formatCount(Number(totals.cancelled)),
                    hint: t.pctOfRuns(Number(totals.runs) ? Math.round((Number(totals.cancelled) / Number(totals.runs)) * 1000) / 10 : 0),
                    meter: Number(totals.runs) ? Number(totals.cancelled) / Number(totals.runs) : 0,
                    tone: Number(totals.cancelled) > 0 ? ("warn" as const) : null,
                  },
                ]
              : []),
            {
              label: t.broken,
              value: formatCount(count("broken")),
              hint: count("broken") ? t.brokenHint : t.noneBroken,
              tone: count("broken") > 0 ? "danger" : null,
              onClick: () => setStatus(status === "broken" ? "all" : "broken"),
              active: status === "broken",
            },
          ]}
        />

        {bars.length ? (
          <div>
            <h2 className="mb-3 mt-0 text-[12.5px] font-semibold text-ink-3">{t.timeBars}</h2>
            <ul className="m-0 flex list-none flex-col gap-2 p-0">
              {bars.map((h) => {
                const tm = h.usage!.timing!;
                const slow = tm.p95_ms > SLOW_P95_MS;
                return (
                  <li key={h.id}>
                    <button
                      type="button"
                      onClick={() => setSelectedId(h.id)}
                      className="grid w-full grid-cols-[minmax(0,240px)_minmax(60px,1fr)_auto] items-center gap-4 rounded-row px-2 py-1 text-left hover:bg-sunk active:scale-[0.995]"
                    >
                      <span className="truncate">
                        <span className="font-semibold tracking-[-0.02em]">{h.name}</span>
                        <span className="text-[12.5px] text-ink-3"> · {facts(h)?.event}</span>
                      </span>
                      <span className="h-2.5 rounded-full bg-sunk">
                        <span
                          className={["block h-full rounded-full", slow ? "bg-warn" : "bg-accent"].join(" ")}
                          style={{
                            width: `${Math.max(1.5, (Number(tm.total_ms) / Number(maxMs)) * 100)}%`,
                          }}
                        />
                      </span>
                      <span className="tnum text-[13px] text-ink-2">
                        <span className="font-semibold text-ink">{formatMs(Number(tm.total_ms))}</span> · p95 {formatMs(tm.p95_ms)}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ) : null}

        <div className="flex flex-wrap items-center gap-2">
          {chip("all", t.all, live.length)}
          {chip("broken", t.broken, count("broken"))}
          {chip("slow", t.slow, count("slow"))}
          {chip("off", t.off, count("off"))}
          {chip("gone", t.gone, count("gone"))}
          <AgentFilter agents={agents} items={live} value={agent} onChange={setAgent} />
          <div className="ml-auto flex items-center gap-3">
            <Segmented value={group} onChange={setGroup} />
          </div>
        </div>

        {isPending ? null : (
          <table className="w-full table-fixed border-collapse text-left text-[14px]">
            <thead>
              <tr className="text-[12.5px] font-semibold text-ink-3">
                <Th table="hooks" label={t.colName} k="name" first="asc" sort={sort} onSort={onSort} className="pr-4" />
                {selected ? null : <Th table="hooks" label={t.colEvent} k="event" first="asc" sort={sort} onSort={onSort} className="w-[190px] pr-4" />}
                {selected ? null : (
                  <Th
                    table="hooks"
                    label={group === "agent" ? t.colOwner : t.agents}
                    k="who"
                    first="asc"
                    sort={sort}
                    onSort={onSort}
                    className="w-[130px] pr-4"
                  />
                )}
                <Th table="hooks" label={t.colRuns} k="runs" sort={sort} onSort={onSort} align="right" resize={!selected} className="w-20 pr-4" />
                {selected ? null : <Th table="hooks" label={t.colP95} k="p95" sort={sort} onSort={onSort} align="right" resize={false} className="w-[130px] pr-2" />}
                {selected ? null : <th className="w-11" aria-label={t.actions} />}
              </tr>
            </thead>
            {groups.map(([key, clusters]) => {
              const label = group === "agent" ? agentName(key) : key || t.noOwner;
              const n = clusters.reduce((a, c) => a + c.items.length, 0);
              const closed = closedGroups.has(key);
              return (
                <tbody key={key || "-"}>
                  <tr className="group-row">
                    <th colSpan={selected ? 2 : 6} className="text-left">
                      <GroupToggle
                        label={label}
                        count={n}
                        open={!closed}
                        onToggle={() => flip(setClosedGroups, key)}
                        icon={group === "agent" && key ? <AgentLogo agent={key} size={15} /> : undefined}
                      />
                    </th>
                  </tr>
                  {closed
                    ? null
                    : clusters.flatMap((c) => {
                        const row = (h: Item, sub: boolean) => (
                          <HookRow
                            key={h.id}
                            item={h}
                            group={group}
                            agents={agents}
                            sub={sub}
                            compact={selected !== null}
                            active={selected?.id === h.id}
                            onOpen={() => setSelectedId(h.id)}
                            agentName={agentName}
                          />
                        );
                        if (c.items.length === 1) return [row(c.items[0]!, false)];
                        const k = `${key}\u0000${c.key}`;
                        const open = openClusters.has(k);
                        return [
                          <ClusterRow
                            key={k}
                            items={c.items}
                            group={group}
                            agents={agents}
                            compact={selected !== null}
                            open={open}
                            onToggle={() => flip(setOpenClusters, k)}
                            agentName={agentName}
                          />,
                          ...(open ? c.items.map((h) => row(h, true)) : []),
                        ];
                      })}
                </tbody>
              );
            })}
            {groups.length === 0 ? (
              <tbody>
                <tr>
                  <td colSpan={selected ? 2 : 6} className="py-10 text-center text-[13px] text-ink-3">
                    {t.noFilterMatches}
                  </td>
                </tr>
              </tbody>
            ) : null}
          </table>
        )}
      </section>
      {selected ? (
        <SheetSteps ids={groups.flatMap(([, g]) => g.flatMap((c) => c.items.map((h) => h.id)))} current={selectedId} go={setSelectedId}>
          <HookSheet
            key={selected.id}
            item={selected}
            agentName={agentName}
            siblings={hooks.filter((h) => h.id !== selected.id && clusterKey(h) === clusterKey(selected) && h.agents[0] === selected.agents[0])}
            onClose={() => setSelectedId(null)}
          />
        </SheetSteps>
      ) : null}
    </div>
  );
}

function Segmented({ value, onChange }: { value: GroupBy; onChange: (g: GroupBy) => void }) {
  return (
    <div role="radiogroup" aria-label={t.groupBy} className="inline-flex h-8 items-center rounded-full bg-sunk p-0.5 text-[12.5px] font-medium">
      {(["agent", "owner"] as const).map((g) => (
        <button
          key={g}
          type="button"
          role="radio"
          aria-checked={value === g}
          onClick={() => onChange(g)}
          className={["h-7 rounded-full px-3", value === g ? "bg-bg text-ink shadow-card" : "text-ink-2"].join(" ")}
        >
          {g === "agent" ? t.groupAgent : t.groupOwner}
        </button>
      ))}
    </div>
  );
}

function StatusChip({ state }: { state: string }) {
  if (state === "ok") return null;
  const tone =
    state === "broken"
      ? "bg-danger-soft text-danger"
      : state === "slow"
        ? "bg-sunk text-warn-text"
        : state === "gone"
          ? "bg-sunk text-ink-3"
          : "bg-sunk text-ink-3";
  return (
    <span title={state === "gone" ? t.goneHookTip : undefined} className={["ml-2 rounded-full px-2 py-0.5 text-[11.5px] font-medium", tone].join(" ")}>
      {t.status[state]}
    </span>
  );
}

/** Hooks that run the same command for one agent (one script on 14 events) fold into one row. */
function clusterKey(h: Item): string {
  const f = facts(h);
  return `${h.name}\u0000${f?.owner ?? ""}\u0000${f?.plugin?.id ?? ""}\u0000${f?.project ?? ""}`;
}

function cluster(list: Item[]): { key: string; items: Item[] }[] {
  const m = new Map<string, Item[]>();
  for (const h of list) {
    const k = clusterKey(h);
    m.set(k, [...(m.get(k) ?? []), h]);
  }
  return [...m.entries()].map(([key, items]) => ({ key, items }));
}

const canRemove = (h: Item) => {
  const f = facts(h);
  const st = h.status.state;
  return !!f && (f.switch === "remove" || f.switch === "native") && st !== "gone" && st !== "unreadable";
};

const STATE_RANK: Record<string, number> = { broken: 4, slow: 3, off: 2, gone: 1 };

function TrashGlyph() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5" />
    </svg>
  );
}

/**
 * Configured hooks SkillDesk cannot take out itself (plugin hooks, OpenCode
 * plugin files). Gone hooks are not here: they are already removed.
 */
const needsAgent = (h: Item) => !canRemove(h) && h.status.state !== "unreadable" && h.status.state !== "gone";

/**
 * A prompt the user pastes in the hook's agent: it finds each hook by event and
 * command and removes it, showing the file and entry first.
 */
function removalPrompt(items: Item[], agentName: (k: string) => string): string {
  const agent = agentName(items[0]?.agents[0] ?? "");
  const lines = items.map((h) => {
    const f = facts(h);
    const m = f?.matcher && f.matcher !== "*" ? ` (matcher ${f.matcher})` : "";
    return `- ${f?.event ?? ""}${m}: ${f?.command ?? h.name}`;
  });
  const plugins = [...new Set(items.map((h) => facts(h)?.plugin?.id).filter((x): x is string => !!x))];
  return [
    `Remove ${items.length === 1 ? "this hook" : `these ${items.length} hooks`} from my ${agent} setup. Each line is the event, then the command it runs:`,
    "",
    ...lines,
    "",
    `Look in every place ${agent} reads hooks from: user, project and local settings files, managed settings, and installed plugins.`,
    plugins.length ? `They come from the plugin ${plugins.join(", ")}. If the plugin cannot drop only these hooks, tell me how to turn the plugin off.` : "",
    "Show me each file and the exact entry before you change it. Change nothing else. If you cannot find a hook, say so.",
  ]
    .filter((l, i, a) => l !== "" || a[i - 1] !== "")
    .join("\n");
}

function useCopied(): [boolean, (text: string) => void] {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const id = window.setTimeout(() => setCopied(false), 1600);
    return () => window.clearTimeout(id);
  }, [copied]);
  return [copied, (text) => void navigator.clipboard.writeText(text).then(() => setCopied(true))];
}

function CopyButton({ text, label, primary }: { text: string; label: string; primary?: boolean }) {
  const [copied, copy] = useCopied();
  return (
    <button
      type="button"
      onClick={() => copy(text)}
      className={[
        "inline-flex h-8 items-center gap-1.5 rounded-full px-3 text-[12.5px] font-semibold active:scale-[0.97]",
        primary ? "bg-ink text-bg" : "bg-bg ring-1 ring-rule hover:bg-sunk",
      ].join(" ")}
    >
      {copied ? <CheckGlyph /> : <CopyGlyph />}
      <span aria-live="polite">{copied ? t.copied : label}</span>
    </button>
  );
}

function CopyGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" aria-hidden="true">
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
      <path d="M10.5 5.5V4A1.5 1.5 0 009 2.5H4A1.5 1.5 0 002.5 4v5A1.5 1.5 0 004 10.5h1.5" />
    </svg>
  );
}

function CheckGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M3.5 8.5l3 3 6-7" />
    </svg>
  );
}

/**
 * Row action, on row hover and on focus: Remove for hooks SkillDesk can take
 * out (Undo in the toast); for the rest, copy a prompt that removes them in their agent.
 */
function RowAction({ items, agentName, label }: { items: Item[]; agentName: (k: string) => string; label: string }) {
  const { run } = useAction();
  const [copied, copy] = useCopied();
  const removable = items.filter(canRemove);
  const other = items.filter(needsAgent);
  if (!removable.length && !other.length) return null;
  const agent = agentName(items[0]?.agents[0] ?? "");
  const hint = removable.length ? t.removeHookNote : t.copyRemovePrompt(agent);
  return (
    <button
      type="button"
      title={copied ? t.copied : hint}
      aria-label={removable.length ? label : t.copyRemovePrompt(agent)}
      onClick={(e) => {
        e.stopPropagation();
        if (removable.length) void run({ type: "hook_remove", ids: removable.map((h) => h.id) });
        else copy(removalPrompt(other, agentName));
      }}
      className={[
        "inline-flex h-7 w-7 items-center justify-center rounded-input text-ink-3 transition-opacity duration-100 focus-visible:opacity-100 active:scale-[0.92] group-hover/row:opacity-100",
        copied ? "opacity-100 text-ok" : "opacity-0",
        removable.length ? "hover:bg-danger-soft hover:text-danger" : "hover:bg-sunk hover:text-ink",
      ].join(" ")}
    >
      {removable.length ? <TrashGlyph /> : copied ? <CheckGlyph /> : <CopyGlyph />}
    </button>
  );
}

function ClusterRow({
  items,
  group,
  agents,
  compact,
  open,
  onToggle,
  agentName,
}: {
  items: Item[];
  group: GroupBy;
  agents: AgentSummary[];
  compact: boolean;
  open: boolean;
  onToggle: () => void;
  agentName: (k: string) => string;
}) {
  const first = items[0]!;
  const f = facts(first);
  const events = items.map((h) => facts(h)?.event ?? "");
  const runs = items.reduce((a, h) => a + Number(h.usage?.count ?? 0), 0);
  const timed = items.map((h) => h.usage?.timing).filter((x): x is NonNullable<typeof x> => !!x);
  const p95 = timed.length ? Math.max(...timed.map((x) => x.p95_ms)) : null;
  const p50 = timed.length ? Math.min(...timed.map((x) => x.p50_ms)) : null;
  const worst = items.map((h) => h.status.state).sort((a, b) => (STATE_RANK[b] ?? 0) - (STATE_RANK[a] ?? 0))[0] ?? "ok";
  const agent = agents.find((a) => a.key === first.agents[0]);
  return (
    <tr onClick={onToggle} className="group/row cursor-default border-t border-rule-2 hover:bg-sunk">
      <td className="truncate py-2.5 pr-4">
        <button type="button" aria-expanded={open} aria-label={open ? t.collapseRow(first.name) : t.expandRow(first.name)} onClick={(e) => { e.stopPropagation(); onToggle(); }} className="-ml-1 inline-flex items-center gap-1.5 font-semibold tracking-[-0.02em] text-ink">
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" className={["shrink-0 text-ink-3 transition-transform duration-150", open ? "rotate-90" : ""].join(" ")}>
            <path d="M4.5 2.5L8 6l-3.5 3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          {first.name}
        </button>
        <span className="tnum ml-2 rounded-full bg-sunk px-2 py-0.5 text-[11.5px] font-medium text-ink-2">{t.eventsN(items.length)}</span>
        <StatusChip state={worst} />
        {compact ? <div className="truncate pl-[18px] text-[12.5px] text-ink-3">{events.join(", ")}</div> : null}
      </td>
      {compact ? null : (
        <td className="truncate py-2.5 pr-4 text-ink-2" title={events.join(", ")}>
          {events.slice(0, 3).join(", ")}
          {events.length > 3 ? <span className="text-ink-3"> +{events.length - 3}</span> : null}
        </td>
      )}
      {compact ? null : (
        <td className="truncate py-2.5 pr-4 text-ink-2">
          {group === "agent" ? (
            (f?.owner ?? "")
          ) : (
            <span className="inline-flex items-center gap-2">
              {first.agents[0] ? <AgentLogo agent={first.agents[0]} size={16} /> : null}
              {agent?.display_name ?? first.agents[0] ?? ""}
            </span>
          )}
        </td>
      )}
      <td className="tnum py-2.5 pr-4 text-right">{runs ? formatCount(runs) : ""}</td>
      {compact ? null : <td className="tnum py-2.5 pr-2 text-right text-ink-2">{p50 !== null && p95 !== null ? `${formatMs(p50)} / ${formatMs(p95)}` : ""}</td>}
      {compact ? null : (
        <td className="py-1 text-right">
          <RowAction items={items} agentName={agentName} label={t.removeHooksN(items.length)} />
        </td>
      )}
    </tr>
  );
}

function HookRow({
  item,
  group,
  agents,
  sub,
  compact,
  active,
  onOpen,
  agentName,
}: {
  item: Item;
  group: GroupBy;
  agents: AgentSummary[];
  sub: boolean;
  compact: boolean;
  active: boolean;
  onOpen: () => void;
  agentName: (k: string) => string;
}) {
  const f = facts(item);
  const tm = item.usage?.timing;
  const agent = agents.find((a) => a.key === item.agents[0]);
  return (
    <tr
      data-row-id={item.id}
      onClick={onOpen}
      className={["group/row cursor-default border-t border-rule-2", sub ? "sub-row" : "", active ? "bg-accent-soft" : "hover:bg-sunk"].join(" ")}
    >
      <td className="truncate py-2.5 pr-4">
        <button type="button" onClick={onOpen} className={sub ? "text-[13.5px] font-medium text-ink-2" : "font-semibold tracking-[-0.02em] text-ink"} title={f?.command}>
          {sub ? f?.event : item.name}
        </button>
        <StatusChip state={item.status.state} />
        {compact && !sub ? <div className="truncate text-[12.5px] text-ink-3">{f?.event}</div> : null}
      </td>
      {compact ? null : (
        <td className="truncate py-2.5 pr-4 text-ink-2">
          {sub ? "" : f?.event}
          {f?.matcher && f.matcher !== "*" ? <span className="text-ink-3">{sub ? "" : " · "}{f.matcher}</span> : null}
        </td>
      )}
      {compact ? null : (
        <td className="truncate py-2.5 pr-4 text-ink-2">
          {sub ? null : group === "agent" ? (
            (f?.owner ?? "")
          ) : (
            <span className="inline-flex items-center gap-2">
              {item.agents[0] ? <AgentLogo agent={item.agents[0]} size={16} /> : null}
              {agent?.display_name ?? item.agents[0] ?? ""}
            </span>
          )}
        </td>
      )}
      <td className="tnum py-2.5 pr-4 text-right">{item.usage ? formatCount(Number(item.usage.count)) : ""}</td>
      {compact ? null : <td className="tnum py-2.5 pr-2 text-right text-ink-2">{tm ? `${formatMs(tm.p50_ms)} / ${formatMs(tm.p95_ms)}` : ""}</td>}
      {compact ? null : (
        <td className="py-1 text-right">
          <RowAction items={[item]} agentName={agentName} label={t.removeHook} />
        </td>
      )}
    </tr>
  );
}

function HookSheet({
  item,
  agentName,
  siblings,
  onClose,
}: {
  item: Item;
  agentName: (k: string) => string;
  /** Same command on other events for this agent. */
  siblings: Item[];
  onClose: () => void;
}) {
  const f = facts(item);
  const tm = item.usage?.timing;
  const { run } = useAction();
  const st = item.status.state;
  const canSwitch = f && f.switch === "native" && st !== "gone" && st !== "unreadable";
  const removable = canRemove(item);
  const switchNote = f
    ? {
        native: t.switchNative,
        remove: t.switchRemove,
        plugin: t.switchPlugin,
        none: t.switchNone,
      }[f.switch]
    : null;
  return (
    <DetailSheet title={item.name} onClose={onClose} width={480}>
      <h2 className="m-0 break-words text-[24px] font-bold leading-tight tracking-[-0.03em]">
        {item.name}
        <StatusChip state={item.status.state} />
      </h2>
      <p className="mt-2 text-ink-2">
        {item.agents.map(agentName).join(", ")} · {f?.event}
        {f?.matcher ? ` · ${f.matcher}` : ""}
        {f?.owner ? ` · ${f.owner}` : ""}
      </p>
      {item.status.state === "unreadable" ? <p className="text-danger">{item.status.reason}</p> : null}
      {canSwitch ? (
        <button
          type="button"
          onClick={() =>
            void run({
              type: "hook_set_enabled",
              ids: [item.id],
              on: st === "off",
            })
          }
          className={[
            "mt-4 h-10 rounded-full px-5 font-semibold active:scale-[0.97]",
            st === "off" ? "bg-ink text-bg" : "bg-bg ring-1 ring-rule hover:bg-sunk",
          ].join(" ")}
        >
          {st === "off" ? t.turnOn : t.turnOff}
        </button>
      ) : null}
      {removable ? (
        <button
          type="button"
          title={t.removeHookNote}
          onClick={() => void run({ type: "hook_remove", ids: [item.id] }, onClose)}
          className={[canSwitch ? "ml-2" : "", "mt-4 h-10 rounded-full px-5 font-semibold text-danger ring-1 ring-rule hover:bg-danger-soft active:scale-[0.97]"].join(" ")}
        >
          {t.removeHook}
        </button>
      ) : null}
      {st === "gone" ? <p className="mt-4 rounded-row bg-sunk px-3 py-2.5 text-[13px] leading-relaxed text-ink-2">{t.goneHookNote}</p> : null}
      {needsAgent(item) ? <AgentRemoval items={[item, ...siblings.filter(needsAgent)]} agentName={agentName} /> : null}
      {removable && siblings.filter(canRemove).length ? (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <span className="text-[12.5px] text-ink-3">{t.eventsN(siblings.length + 1)}:</span>
          {canSwitch ? (
            <button
              type="button"
              onClick={() => void run({ type: "hook_set_enabled", ids: [item, ...siblings.filter(canRemove)].map((h) => h.id), on: st === "off" })}
              className="h-8 rounded-full bg-bg px-3 text-[12.5px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
            >
              {st === "off" ? t.turnOnAll : t.turnOffAll}
            </button>
          ) : null}
          <button
            type="button"
            title={t.removeHookNote}
            onClick={() => void run({ type: "hook_remove", ids: [item, ...siblings.filter(canRemove)].map((h) => h.id) }, onClose)}
            className="h-8 rounded-full px-3 text-[12.5px] font-semibold text-danger ring-1 ring-rule hover:bg-danger-soft active:scale-[0.97]"
          >
            {t.removeHooksN(siblings.filter(canRemove).length + 1)}
          </button>
        </div>
      ) : null}
      {tm ? (
        <dl className="mt-4 grid grid-cols-3 gap-x-6 gap-y-3">
          <SheetStat label={t.total} value={formatMs(Number(tm.total_ms))} />
          <SheetStat label={t.p50} value={formatMs(tm.p50_ms)} />
          <SheetStat label={t.p95} value={formatMs(tm.p95_ms)} warn={tm.p95_ms > SLOW_P95_MS} />
          <SheetStat label={t.max} value={formatMs(tm.max_ms)} />
          {tm.cancelled ? <SheetStat label={t.cancelled} value={formatCount(tm.cancelled)} /> : null}
          {tm.failed ? <SheetStat label={t.failed} value={formatCount(tm.failed)} /> : null}
          {tm.blocked ? <SheetStat label={t.blocked} value={formatCount(tm.blocked)} /> : null}
        </dl>
      ) : (
        <p className="mt-4 text-[13px] text-ink-3">{t.noRunData}</p>
      )}
      {f?.missing_paths.length ? (
        <>
          <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-danger">{t.missingScripts}</h3>
          <ul className="selectable m-0 flex list-none flex-col gap-1 rounded-row bg-danger-soft p-3 font-mono text-[12.5px] text-danger">
            {f.missing_paths.map((p) => (
              <li key={p} className="break-all">
                {tildePath(p)}
              </li>
            ))}
          </ul>
        </>
      ) : null}
      {f ? (
        <>
          <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">{t.command}</h3>
          <pre className="selectable m-0 max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-row bg-sunk p-3 font-mono text-[12px] leading-relaxed">
            {f.command}
          </pre>
        </>
      ) : null}
      <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">{t.configuredIn}</h3>
      {item.sources.length ? (
        <ul className="m-0 flex list-none flex-col gap-2 p-0">
          {item.sources.map((s) => (
            <li key={s.path} className="selectable break-all rounded-row bg-panel px-3 py-2 font-mono text-[12.5px]">
              {tildePath(s.path)}
            </li>
          ))}
        </ul>
      ) : (
        <p className="m-0 text-[13px] text-ink-2">{t.notConfigured}</p>
      )}
      <dl className="mt-4 flex flex-col gap-1 text-[13px] text-ink-2">
        {f?.timeout_ms ? (
          <div>
            {t.timeout} {formatMs(Number(f.timeout_ms))}
          </div>
        ) : null}
        {f?.plugin ? (
          <div>
            {t.plugin} <PluginName id={f.plugin.id} />
            {f.plugin.enabled ? "" : ` (${t.status.off})`}
          </div>
        ) : null}
        {f?.project ? (
          <div>
            {t.project} <PathLink path={f.project}>{tildePath(f.project)}</PathLink>
          </div>
        ) : null}
        {item.status.state !== "gone" && switchNote ? <div>{switchNote}</div> : null}
      </dl>
    </DetailSheet>
  );
}

/** For hooks SkillDesk cannot remove: the prompt, readable and selectable, and one Copy button. */
function AgentRemoval({ items, agentName }: { items: Item[]; agentName: (k: string) => string }) {
  const agent = agentName(items[0]?.agents[0] ?? "");
  const prompt = removalPrompt(items, agentName);
  return (
    <section className="mt-5 rounded-card bg-panel p-4 ring-1 ring-rule">
      <div className="flex items-center gap-3">
        <h3 className="m-0 text-[13px] font-semibold">{t.removeInAgent(agent)}</h3>
        <span className="ml-auto">
          <CopyButton text={prompt} label={t.copyPrompt} primary />
        </span>
      </div>
      <p className="mb-3 mt-1.5 text-[12.5px] leading-relaxed text-ink-2">{t.agentHookNote(agent)}</p>
      <pre className="selectable m-0 max-h-56 overflow-auto whitespace-pre-wrap break-words rounded-input bg-bg p-3 font-mono text-[11.5px] leading-relaxed text-ink-2 ring-1 ring-rule">
        {prompt}
      </pre>
    </section>
  );
}

function SheetStat({ label, value, warn }: { label: string; value: string; warn?: boolean }) {
  return (
    <div>
      <dt className="text-[12.5px] font-semibold text-ink-3">{label}</dt>
      <dd className={["tnum m-0 text-[20px] font-bold tracking-[-0.03em]", warn ? "text-warn-text" : ""].join(" ")}>{value}</dd>
    </div>
  );
}
