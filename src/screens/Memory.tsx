import { SearchField } from "../components/SearchField";
import { Th, sortBy, useSort } from "../components/SortHeader";
import { StatTiles } from "../components/StatTiles";
import { PageHeader } from "../components/PageHeader";
// Memory (DESIGN.md §7.8): Claude memories by project, duplicates clustered,
// stale flags. Edit, Forget (Trash), Copy to agent, Promote / merge into one
// global rule. Every change goes through the action flow (plan → Undo).
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useSearchParams } from "react-router-dom";
import { PathLink } from "../components/PlaceLink";
import { useQuery } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { ipc } from "../ipc/client";
import { useItems, useOverview } from "../ipc/queries";
import { AgentLogo, AgentStack } from "../components/AgentLogo";
import { agentName } from "../lib/agents";
import { getErrorMessage } from "../ipc/error";
import { useAction } from "../actions/ActionProvider";
import type { Item } from "../ipc/bindings/Item";
import type { MemoryFacts } from "../ipc/bindings/MemoryFacts";
import type { RuleTarget } from "../ipc/bindings/RuleTarget";
import { FChip } from "../components/FChip";
import { Menu } from "../components/Menu";
import { GroupToggle } from "../components/GroupToggle";
import { DetailSheet, SheetSteps } from "../components/DetailSheet";
import { formatCount, formatDate, stripFrontmatter, tildePath } from "../lib/format";
import { t } from "../strings";

type Filter = "all" | "instructions" | "feedback" | "project" | "user" | "reference" | "same" | "stale";

const facts = (i: Item): MemoryFacts | null => (i.facts.kind === "memory" ? i.facts : null);
const f = (i: Item) => facts(i)!;

/** First guess for the merged rule: the shortest description in the group. */
function guessRule(items: Item[]): string {
  const ds = items.map((i) => f(i).description ?? i.name.replace(/[-_]/g, " ")).sort((a, b) => a.length - b.length);
  return ds[0] ?? "";
}

export function Memory() {
  const [params, setParams] = useSearchParams();
  const [filter, setFilter] = useState<Filter>("all");
  // Where the list looks (the sidebar): "" every memory, USER the ones that apply everywhere, else one project.
  const [place, setPlace] = useState<string>(() => (params.get("scope") === "user" ? USER : (params.get("project") ?? "")));
  const [agent, setAgent] = useState<string | null>(params.get("agent"));
  const agentList = useOverview().data?.agents ?? [];
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(params.get("id"));
  const [merge, setMerge] = useState<string[] | null>(null);
  const q = useItems("memory");
  const every = useMemo(() => q.data ?? [], [q.data]);
  // The agent narrows everything; the sidebar place narrows the list and its chips.
  const base = useMemo(() => every.filter((i) => !agent || i.agents.includes(agent)), [every, agent]);
  const inPlace = (i: Item) => (place === "" ? true : place === USER ? f(i).scope === "user" : f(i).scope === "project" && f(i).project_label === place);
  const all = base.filter(inPlace);
  const byAgent = agentList.map((a) => ({ a, n: every.filter((i) => inPlace(i) && i.agents.includes(a.key)).length })).filter((x) => x.n > 0);

  // `?cluster=<key>` from the Fix list opens the merge sheet for that group.
  const clusterParam = params.get("cluster");
  useEffect(() => {
    if (!clusterParam || !all.length) return;
    const ids = all.filter((i) => f(i).cluster === clusterParam).map((i) => i.id);
    if (ids.length) {
      setSelectedId(null);
      setMerge(ids);
    }
  }, [clusterParam, all]);
  useEffect(() => setSelectedId(params.get("id")), [params]);

  const clusters = useMemo(() => {
    const m = new Map<string, Item[]>();
    for (const i of all) {
      const c = f(i).cluster;
      if (c) m.set(c, [...(m.get(c) ?? []), i]);
    }
    return [...m.values()].sort((a, b) => b.length - a.length);
  }, [all]);
  const n = {
    all: all.length,
    feedback: all.filter((i) => f(i).mem_type === "feedback").length,
    project: all.filter((i) => f(i).mem_type === "project").length,
    user: all.filter((i) => f(i).mem_type === "user").length,
    instructions: all.filter((i) => f(i).mem_type === "instructions").length,
    reference: all.filter((i) => f(i).mem_type === "reference").length,
    same: all.filter((i) => f(i).cluster).length,
    stale: all.filter((i) => f(i).stale.length).length,
  };
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const projects = useMemo(() => {
    const m = new Map<string, number>();
    for (const i of base) if (f(i).scope === "project") m.set(f(i).project_label, (m.get(f(i).project_label) ?? 0) + 1);
    return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0], undefined, { sensitivity: "base" }));
  }, [base]);
  const userN = base.filter((i) => f(i).scope === "user").length;
  // A place that empties (agent filter, a forget) falls back to every memory.
  useEffect(() => {
    if (place && place !== USER && every.length && !projects.some(([p]) => p === place)) setPlace("");
  }, [place, projects, every.length]);
  const shown = all.filter(
    (i) =>
      (filter === "all" ? true : filter === "same" ? f(i).cluster : filter === "stale" ? f(i).stale.length : f(i).mem_type === filter) &&
      (!query || `${i.name} ${f(i).project_label} ${f(i).description ?? ""}`.toLowerCase().includes(query.trim().toLowerCase())),
  );
  // One place shows a flat list; every memory groups by project.
  const grouped = place === "";
  const toggleGroup = (label: string) =>
    setCollapsed((s) => {
      const next = new Set(s);
      if (next.has(label)) next.delete(label);
      else next.add(label);
      return next;
    });
  const { sort, toggle: onSort } = useSort<"type" | "name" | "agents" | "modified">();
  const groups = useMemo(() => {
    const m = new Map<string, Item[]>();
    // Sorting orders memories inside each project; projects stay A to Z.
    const ordered = sortBy(shown, sort, (i, k) =>
      k === "type" ? f(i).mem_type : k === "name" ? i.name : k === "agents" ? i.agents.length : (f(i).modified ?? null),
    );
    const label = (i: Item) => (f(i).scope === "user" ? t.everywhere : f(i).project_label);
    for (const i of ordered) m.set(label(i), [...(m.get(label(i)) ?? []), i]);
    // Memories that apply everywhere first, then projects A to Z.
    return [...m.entries()].sort((a, b) => Number(b[0] === t.everywhere) - Number(a[0] === t.everywhere) || a[0].localeCompare(b[0], undefined, { sensitivity: "base" }));
  }, [shown, sort]);
  const placeLabel = place === "" ? t.allMemories : place === USER ? t.everywhere : place;
  const selected = all.find((i) => i.id === selectedId) ?? null;
  const mergeItems = merge ? all.filter((i) => merge.includes(i.id)) : [];

  const chip = (k: Filter, label: string) =>
    n[k] || k === "all" ? <FChip label={label} count={n[k]} active={filter === k} onClick={() => setFilter(filter === k && k !== "all" ? "all" : k)} /> : null;
  const closeMerge = () => {
    setMerge(null);
    if (clusterParam) setParams({});
  };
  const open = (id: string) => {
    setMerge(null);
    setSelectedId(id);
  };

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-6 overflow-auto page">
        <PageHeader
          title={t.memoryTitle}
          search={<SearchField value={query} onChange={setQuery} label={t.searchMemory} count={query ? t.nResults(shown.length) : null} className="w-64" />}
        />
        {q.error ? (
          <p role="alert" className="text-danger">
            {getErrorMessage(q.error, t.loadFailed)}
          </p>
        ) : null}
        <StatTiles
          label={t.memoryTitle}
          items={[
            {
              label: t.memoriesLabel,
              value: formatCount(base.length),
              split: [
                { label: t.scopeUser, value: userN, tone: "accent" },
                { label: t.scopeProject, value: base.length - userN, tone: "muted" },
              ],
            },
            {
              label: t.memProjects,
              value: formatCount(projects.length),
              bars: projects.map(([p, c]) => ({ label: p, value: c })),
            },
            {
              label: t.sameRule,
              value: formatCount(n.same),
              hint: n.same ? t.sameRuleHint : t.noDuplicates,
              tone: n.same > 0 ? "warn" : null,
              onClick: () => setFilter(filter === "same" ? "all" : "same"),
              active: filter === "same",
            },
            {
              label: t.stale,
              value: formatCount(n.stale),
              hint: n.stale ? t.staleHint : t.nothingStale,
              tone: n.stale > 0 ? "warn" : null,
              onClick: () => setFilter(filter === "stale" ? "all" : "stale"),
              active: filter === "stale",
            },
          ]}
        />
        {clusters.map((c) => (
          <div key={f(c[0]!).cluster} className="flex items-center gap-4 rounded-row bg-panel px-4 py-3">
            <div className="min-w-0">
              <p className="m-0 font-semibold tracking-[-0.02em]">{t.sameRuleTitle(c.length)}</p>
              <p className="m-0 mt-0.5 truncate text-[13px] text-ink-3">
                {[...new Set(c.map((i) => f(i).project_label))].join(", ")} · {guessRule(c)}
              </p>
            </div>
            <button
              type="button"
              onClick={() => {
                setSelectedId(null);
                setMerge(c.map((i) => i.id));
              }}
              className="ml-auto h-8 shrink-0 rounded-full bg-ink px-4 text-[13px] font-semibold text-bg active:scale-[0.97]"
            >
              {t.mergeRule}
            </button>
          </div>
        ))}
        <div className="flex min-w-0 items-start gap-8">
          <PlaceList
            place={place}
            onPick={setPlace}
            total={base.length}
            userN={userN}
            projects={projects}
          />
          <div className="flex min-w-0 flex-1 flex-col gap-4">
            <div className="flex flex-wrap items-center gap-2">
              {/* Narrow windows: the sidebar folds into this menu. */}
              <div className="lg:hidden">
                <Menu
                  label={t.projectFilter}
                  value={place}
                  onChange={setPlace}
                  icon={<FolderGlyph />}
                  options={[
                    { value: "", label: t.allMemories, hint: String(base.length) },
                    ...(userN ? [{ value: USER, label: t.everywhere, hint: String(userN) }] : []),
                    ...projects.map(([p, c]) => ({ value: p, label: p, hint: String(c) })),
                  ]}
                />
              </div>
              <h2 className="m-0 hidden min-w-0 truncate text-[17px] font-semibold tracking-[-0.02em] lg:block">
                {placeLabel}
                <span className="tnum ml-2 text-[13px] font-normal text-ink-3">{all.length}</span>
              </h2>
              {byAgent.length > 1 || agent ? (
                <div className="ml-auto flex flex-wrap items-center gap-2">
                  {byAgent.map(({ a, n: c }) => (
                    <FChip
                      key={a.key}
                      label={a.display_name}
                      count={c}
                      icon={<AgentLogo agent={a.key} size={14} tone={agent === a.key ? "text-current" : "text-ink"} />}
                      active={agent === a.key}
                      onClick={() => setAgent(agent === a.key ? null : a.key)}
                    />
                  ))}
                </div>
              ) : null}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {chip("all", t.all)}
              {chip("instructions", t.memType.instructions!)}
              {chip("feedback", t.memType.feedback!)}
              {chip("project", t.memType.project!)}
              {chip("user", t.memType.user!)}
              {chip("reference", t.memType.reference!)}
              {chip("same", t.sameRule)}
              {chip("stale", t.stale)}
            </div>
            {q.isPending ? null : (
              <table className="w-full table-fixed border-collapse text-left text-[14px]">
                <thead>
                  <tr className="text-[12.5px] font-semibold text-ink-3">
                    <Th table="memory" label={t.colType} k="type" first="asc" sort={sort} onSort={onSort} className="w-28 pr-3" />
                    <Th table="memory" label={t.colName} k="name" first="asc" sort={sort} onSort={onSort} resize={!(selected || merge)} className="pr-4" />
                    {selected || merge ? null : <Th table="memory" label={t.agentsCol} k="agents" sort={sort} onSort={onSort} className="w-[130px] pr-4" />}
                    {selected || merge ? null : <Th table="memory" label={t.modified} k="modified" sort={sort} onSort={onSort} resize={false} className="w-28" />}
                  </tr>
                </thead>
                {groups.map(([label, items]) => (
                  <tbody key={label}>
                    {grouped ? (
                      <tr className="group-row">
                        <th colSpan={selected || merge ? 2 : 4} className="text-left">
                          <GroupToggle label={label} count={items.length} open={!collapsed.has(label)} onToggle={() => toggleGroup(label)} />
                        </th>
                      </tr>
                    ) : null}
                    {grouped && collapsed.has(label)
                      ? null
                      : items.map((i) => (
                          <Row
                            key={i.id}
                            item={i}
                            active={selected?.id === i.id || (merge?.includes(i.id) ?? false)}
                            compact={selected !== null || merge !== null}
                            onOpen={() => open(i.id)}
                            name={(k) => agentName(agentList, k)}
                          />
                        ))}
                  </tbody>
                ))}
                {groups.length === 0 ? (
                  <tbody>
                    <tr>
                      <td colSpan={selected || merge ? 2 : 4} className="py-10 text-center text-[13px] text-ink-3">
                        {t.noFilterMatches}
                      </td>
                    </tr>
                  </tbody>
                ) : null}
              </table>
            )}
          </div>
        </div>
        <p className="m-0 text-[12.5px] text-ink-3">{t.memoryNote}</p>
      </section>
      {merge && mergeItems.length ? (
        <MergeSheet key={merge.join(",")} items={mergeItems} onClose={closeMerge} />
      ) : selected ? (
        <SheetSteps ids={groups.flatMap(([, g]) => g.map((i) => i.id))} current={selectedId} go={setSelectedId}>
          <MemorySheet key={selected.id} item={selected} onClose={() => setSelectedId(null)} onPromote={() => setMerge([selected.id])} />
        </SheetSteps>
      ) : null}
    </div>
  );
}

const USER = "\u0000user";

function FolderGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" aria-hidden="true" className="shrink-0">
      <path d="M2 4.5A1.5 1.5 0 013.5 3h2.6l1.4 1.5h5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z" />
    </svg>
  );
}

function StackGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" aria-hidden="true" className="shrink-0">
      <path d="M8 2.5l5.5 3L8 8.5l-5.5-3z" />
      <path d="M2.5 8.5L8 11.5l5.5-3" strokeLinecap="round" />
    </svg>
  );
}

function GlobeGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true" className="shrink-0">
      <circle cx="8" cy="8" r="5.5" />
      <path d="M2.5 8h11M8 2.5c1.6 1.6 2.3 3.4 2.3 5.5S9.6 11.9 8 13.5M8 2.5C6.4 4.1 5.7 5.9 5.7 8s.7 3.9 2.3 5.5" />
    </svg>
  );
}

/**
 * The source list (a macOS sidebar): every memory, the ones that apply
 * everywhere, then one row per project. It stays in view while the list
 * scrolls; with many projects a filter field narrows it.
 */
function PlaceList({
  place,
  onPick,
  total,
  userN,
  projects,
}: {
  place: string;
  onPick: (p: string) => void;
  total: number;
  userN: number;
  projects: [string, number][];
}) {
  const [find, setFind] = useState("");
  const list = find ? projects.filter(([p]) => p.toLowerCase().includes(find.trim().toLowerCase())) : projects;
  const row = (value: string, label: string, n: number, icon: ReactNode) => {
    const on = place === value;
    return (
      <li key={value}>
        <button
          type="button"
          aria-current={on ? "true" : undefined}
          onClick={() => onPick(value)}
          className={[
            "flex h-8 w-full items-center gap-2.5 rounded-input px-2.5 text-left text-[13px] transition-colors duration-100 active:scale-[0.99]",
            on ? "bg-sunk font-semibold text-ink ring-1 ring-rule" : "text-ink-2 hover:bg-sunk/60 hover:text-ink",
          ].join(" ")}
        >
          <span className={on ? "text-accent-text" : "text-ink-3"}>{icon}</span>
          <span className="min-w-0 flex-1 truncate">{label}</span>
          <span className="tnum text-[12px] font-normal text-ink-3">{n}</span>
        </button>
      </li>
    );
  };
  return (
    <nav aria-label={t.projectFilter} className="sticky top-0 hidden max-h-[calc(100vh-120px)] w-56 shrink-0 flex-col gap-4 overflow-auto pb-4 lg:flex xl:w-64">
      <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
        {row("", t.allMemories, total, <StackGlyph />)}
        {userN ? row(USER, t.everywhere, userN, <GlobeGlyph />) : null}
      </ul>
      {projects.length ? (
        <div className="flex flex-col gap-1.5">
          <h3 className="m-0 px-2.5 text-[11.5px] font-semibold text-ink-3">
            {t.memProjects}
            <span className="tnum ml-1.5 font-normal">{projects.length}</span>
          </h3>
          {projects.length > 8 ? (
            <input
              type="search"
              value={find}
              onChange={(e) => setFind(e.target.value)}
              placeholder={t.findProject}
              aria-label={t.findProject}
              className="mx-0.5 h-7 rounded-input bg-panel px-2.5 text-[12.5px] outline-none ring-1 ring-rule placeholder:text-ink-3 focus:ring-ink"
            />
          ) : null}
          <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
            {list.map(([p, c]) => row(p, p, c, <FolderGlyph />))}
            {list.length === 0 ? <li className="px-2.5 py-1.5 text-[12.5px] text-ink-3">{t.noFilterMatches}</li> : null}
          </ul>
        </div>
      ) : null}
    </nav>
  );
}

function TypeChip({ item }: { item: Item }) {
  const ty = f(item).mem_type;
  return (
    <span className={["rounded-full px-2 py-0.5 text-[11.5px] font-medium", ty === "feedback" ? "bg-accent-soft text-ink-2" : "bg-sunk text-ink-3"].join(" ")}>
      {t.memType[ty]}
    </span>
  );
}

function Badges({ item }: { item: Item }) {
  const m = f(item);
  const pill = (label: string, tone: string) => <span className={["ml-2 rounded-full px-2 py-0.5 text-[11.5px] font-medium", tone].join(" ")}>{label}</span>;
  return (
    <>
      {m.cluster ? pill(t.sameRule, "bg-sunk text-warn-text") : null}
      {m.stale.length ? pill(t.stale, "bg-sunk text-warn-text") : null}
    </>
  );
}

function Row({ item, active, compact, onOpen, name }: { item: Item; active: boolean; compact: boolean; onOpen: () => void; name: (k: string) => string }) {
  const m = f(item);
  return (
    <tr data-row-id={item.id} onClick={onOpen} className={["cursor-default border-t border-rule-2", active ? "bg-accent-soft" : "hover:bg-sunk"].join(" ")}>
      <td className="py-3 pr-3 align-top">
        <TypeChip item={item} />
      </td>
      <td className="min-w-0 py-3 pr-4">
        <div className="truncate">
          <button type="button" onClick={onOpen} className="font-semibold tracking-[-0.02em]">
            {item.name}
          </button>
          <Badges item={item} />
        </div>
        {m.description ? <div className="truncate text-[13px] text-ink-3">{m.description}</div> : null}
      </td>
      {compact ? null : (
        <td className="py-3 pr-4 align-top">
          <AgentStack agents={item.agents} name={name} size={22} card={false} />
        </td>
      )}
      {compact ? null : <td className="tnum py-3 align-top text-ink-2">{formatDate(m.modified)}</td>}
    </tr>
  );
}

function StaleNotes({ item }: { item: Item }) {
  const m = f(item);
  const notes = m.stale.map((s) => (s.kind === "repo_gone" ? t.staleRepo : t.stalePast(s.date)));
  if (!m.index) notes.push(t.notInIndex);
  return notes.length ? (
    <ul className="m-0 mt-3 flex list-none flex-col gap-1 p-0">
      {notes.map((x) => (
        <li key={x} className="rounded-row bg-sunk px-3 py-2 text-[13px] text-warn-text">
          {x}
        </li>
      ))}
    </ul>
  ) : null;
}

const btn = "h-8 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97] disabled:opacity-50";
const primary = "h-8 rounded-full bg-ink px-4 text-[13px] font-semibold text-bg active:scale-[0.97] disabled:opacity-50";

function MemorySheet({ item, onClose, onPromote }: { item: Item; onClose: () => void; onPromote: () => void }) {
  const m = f(item);
  const agentList = useOverview().data?.agents ?? [];
  const { run } = useAction();
  const path = item.sources[0]?.path ?? "";
  const doc = useQuery({
    queryKey: ["doc", item.id],
    queryFn: () => ipc.itemDocument(item.id),
  });
  const targets = useQuery({
    queryKey: ["ruleTargets"],
    queryFn: ipc.memoryRuleTargets,
  });
  const isFile = m.mem_type === "instructions";
  const copyTargets = isFile ? [] : (targets.data ?? []).filter((x) => x.agent !== "claude_code");
  const [editing, setEditing] = useState<string | null>(null);
  const [copyTo, setCopyTo] = useState<string | null>(null);
  const [rule, setRule] = useState(m.description ?? "");

  return (
    <DetailSheet title={item.name} onClose={onClose} width={480}>
      <h2 className="m-0 break-words text-[24px] font-bold leading-tight tracking-[-0.03em]">
        {item.name}
        <Badges item={item} />
      </h2>
      <p className="mt-2 flex items-center gap-2 text-[13px] text-ink-3">
        <TypeChip item={item} />
        <AgentStack agents={item.agents} name={(k) => agentName(agentList, k)} size={20} card={false} />
        {isFile || m.project ? (
          <span className="flex min-w-0">
            <PathLink path={isFile ? path : m.project!}>{tildePath(isFile ? path : m.project!)}</PathLink>
          </span>
        ) : (
          <span className="truncate">{m.project_label}</span>
        )}
        {m.modified ? <span>· {formatDate(m.modified)}</span> : null}
      </p>
      {m.description ? <p className="mt-2 text-ink-2">{m.description}</p> : null}
      <StaleNotes item={item} />
      <div className="mt-4 flex flex-wrap gap-2">
        <button type="button" className={btn} disabled={!doc.data} onClick={() => setEditing(doc.data ?? "")}>
          {t.editMemory}
        </button>
        {isFile ? null : (
          <>
            <button type="button" className={btn} onClick={() => void run({ type: "memory_forget", ids: [item.id] }, onClose)} title={t.forgetNote}>
              {t.forget}
            </button>
            <button type="button" className={btn} onClick={onPromote}>
              {t.promoteRule}
            </button>
          </>
        )}
        <button type="button" onClick={() => void ipc.revealInFinder(path)} className="ml-auto text-[12.5px] font-semibold text-ink-2 hover:text-ink">
          {t.showInFinder}
        </button>
      </div>
      {copyTargets.length ? (
        <div className="mt-4">
          <h3 className="mb-2 mt-0 text-[12.5px] font-semibold text-ink-3">{t.copyTo}</h3>
          <div className="flex flex-wrap gap-2">
            {copyTargets.map((x) => (
              <button
                key={x.agent}
                type="button"
                aria-pressed={copyTo === x.agent}
                title={tildePath(x.path)}
                onClick={() => setCopyTo(copyTo === x.agent ? null : x.agent)}
                className={[
                  "h-8 rounded-full px-3 text-[13px] font-medium active:scale-[0.97]",
                  copyTo === x.agent ? "bg-ink text-bg" : "bg-bg ring-1 ring-rule hover:bg-sunk",
                ].join(" ")}
              >
                {x.display_name}
              </button>
            ))}
          </div>
          {copyTo ? (
            <div className="mt-3">
              <RuleField value={rule} onChange={setRule} />
              <p className="mb-2 mt-2 text-[12.5px] text-ink-3">{t.copyNote}</p>
              <button
                type="button"
                className={primary}
                disabled={!rule.trim()}
                onClick={() =>
                  void run(
                    {
                      type: "memory_copy_to_agent",
                      id: item.id,
                      agent: copyTo,
                      text: rule,
                    },
                    () => setCopyTo(null),
                  )
                }
              >
                {t.copyTo} {copyTargets.find((x) => x.agent === copyTo)?.display_name}
              </button>
            </div>
          ) : null}
        </div>
      ) : null}
      <div className="selectable mt-4 truncate font-mono text-[11.5px] text-ink-3" title={path}>
        {tildePath(path)}
      </div>
      {editing !== null ? (
        <div className="mt-4">
          <textarea
            aria-label={t.editMemory}
            value={editing}
            onChange={(e) => setEditing(e.target.value)}
            spellCheck={false}
            className="h-80 w-full resize-y rounded-input bg-panel p-3 font-mono text-[12px] leading-relaxed outline-none ring-1 ring-rule focus:ring-ink"
          />
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              className={primary}
              disabled={editing === doc.data || !editing.trim()}
              onClick={() => void run({ type: "memory_edit", id: item.id, text: editing }, () => setEditing(null))}
            >
              {t.saveEdit}
            </button>
            <button type="button" className={btn} onClick={() => setEditing(null)}>
              {t.cancel}
            </button>
          </div>
        </div>
      ) : (
        <article className="skill-md selectable mt-6 border-t border-rule pt-4 text-[14px]">
          {doc.error ? <p className="text-danger">{getErrorMessage(doc.error, t.loadFailed)}</p> : null}
          {doc.data ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{stripFrontmatter(doc.data)}</ReactMarkdown> : null}
        </article>
      )}
    </DetailSheet>
  );
}

function RuleField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <label className="block">
      <span className="mb-1 block text-[12.5px] font-semibold text-ink-3">{t.ruleText}</span>
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={t.rulePlaceholder}
        rows={3}
        className="w-full resize-y rounded-input bg-panel p-3 text-[14px] outline-none ring-1 ring-rule focus:ring-ink"
      />
    </label>
  );
}

function MergeSheet({ items, onClose }: { items: Item[]; onClose: () => void }) {
  const { run } = useAction();
  const targets = useQuery({
    queryKey: ["ruleTargets"],
    queryFn: ipc.memoryRuleTargets,
  });
  const [picked, setPicked] = useState<string[]>(items.map((i) => i.id));
  const [agents, setAgents] = useState<string[]>(["claude_code"]);
  const [rule, setRule] = useState(guessRule(items));
  const list: RuleTarget[] = targets.data ?? [];
  const single = items.length === 1;
  return (
    <DetailSheet title={t.mergeRule} onClose={onClose} width={480}>
      <h2 className="m-0 text-[24px] font-bold leading-tight tracking-[-0.03em]">{single ? t.promoteRule : t.sameRuleTitle(items.length)}</h2>
      {single ? null : (
        <ul className="m-0 mt-4 flex list-none flex-col gap-2 p-0">
          {items.map((i) => {
            const on = picked.includes(i.id);
            return (
              <li key={i.id} className="rounded-row bg-panel px-3 py-2.5">
                <label className="flex items-start gap-3">
                  <input
                    type="checkbox"
                    aria-label={`${t.includeInMerge} ${i.name}`}
                    checked={on}
                    onChange={() => setPicked(on ? picked.filter((x) => x !== i.id) : [...picked, i.id])}
                    className="mt-1 accent-ink"
                  />
                  <span className="min-w-0">
                    <span className="font-semibold">{f(i).project_label}</span>
                    <span className="ml-2 text-[12.5px] text-ink-3">{i.name}</span>
                    {f(i).description ? <span className="block truncate text-[13px] text-ink-3">{f(i).description}</span> : null}
                  </span>
                </label>
              </li>
            );
          })}
        </ul>
      )}
      <div className="mt-5">
        <RuleField value={rule} onChange={setRule} />
      </div>
      <h3 className="mb-2 mt-5 text-[12.5px] font-semibold text-ink-3">{t.writeTo}</h3>
      {targets.data && !list.length ? <p className="text-[13px] text-ink-3">{t.noRuleTargets}</p> : null}
      <div className="flex flex-col gap-2">
        {list.map((x) => {
          const on = agents.includes(x.agent);
          return (
            <label key={x.agent} className="flex items-center gap-3 rounded-row bg-panel px-3 py-2">
              <input
                type="checkbox"
                className="accent-ink"
                checked={on}
                onChange={() => setAgents(on ? agents.filter((a) => a !== x.agent) : [...agents, x.agent])}
              />
              <span className="font-semibold">{x.display_name}</span>
              <span className="truncate font-mono text-[11.5px] text-ink-3" title={x.path}>
                {tildePath(x.path)}
                {x.exists ? "" : ` · ${t.newFile}`}
              </span>
            </label>
          );
        })}
      </div>
      <p className="mb-0 mt-4 text-[12.5px] text-ink-3">{t.mergeNote}</p>
      <div className="sticky bottom-0 -mx-6 mt-4 border-t border-rule bg-bg px-6 py-3">
        <button
          type="button"
          className={primary}
          disabled={!picked.length || !agents.length || !rule.trim()}
          onClick={() => void run({ type: "memory_promote", ids: picked, agents, text: rule }, onClose)}
        >
          {single ? t.promoteRule : t.mergeRule}
          {picked.length > 1 ? <span className="tnum ml-1.5 font-normal opacity-70">{picked.length}</span> : null}
        </button>
      </div>
    </DetailSheet>
  );
}
