import { SearchField } from "../components/SearchField";
import { StatTiles } from "../components/StatTiles";
import { PageHeader } from "../components/PageHeader";
import { AgentStack } from "../components/AgentLogo";
import { Th, sortBy, useSort } from "../components/SortHeader";
import { useEffect, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { ipc } from "../ipc/client";
import { useItems, useOverview } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import type { Item } from "../ipc/bindings/Item";
import type { DefFacts } from "../ipc/bindings/DefFacts";
import type { AgentSummary } from "../ipc/bindings/AgentSummary";
import { FChip } from "../components/FChip";
import { AgentFilter } from "../components/AgentFilter";
import { PathLink, PlaceLink, usePluginRoute, useSkillRoute } from "../components/PlaceLink";
import { DetailSheet, SheetSteps } from "../components/DetailSheet";
import { agentName } from "../lib/agents";
import { formatCount, formatDate, stripFrontmatter, tildePath } from "../lib/format";
import { t } from "../strings";

type Tab = "subagent" | "command";
type Filter = "all" | "unused" | "built_in" | "skill";

const facts = (i: Item): DefFacts | null => (i.facts.kind === "subagent" || i.facts.kind === "command" ? i.facts : null);
const runs = (i: Item) => Number(i.usage?.count ?? 0);

export function Subagents() {
  const [params, setParams] = useSearchParams();
  const tab: Tab = params.get("tab") === "command" ? "command" : "subagent";
  const [filter, setFilter] = useState<Filter>("all");
  const [agent, setAgent] = useState<string | null>(params.get("agent"));
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(params.get("id"));
  useEffect(() => {
    setSelectedId(params.get("id"));
    setAgent(params.get("agent"));
  }, [params]);
  useEffect(() => setFilter("all"), [tab]);
  const subagents = useItems("subagent");
  const commands = useItems("command");
  const q = tab === "subagent" ? subagents : commands;
  const agents = useOverview().data?.agents ?? [];
  // Row names its agents from the full list (any tier).
  const columns = agents;
  const all = q.data ?? [];
  const known = all.filter((i) => facts(i)?.origin !== "unknown");
  const unknown = all.filter((i) => facts(i)?.origin === "unknown").sort((a, b) => runs(b) - runs(a));
  const selected = all.find((i) => i.id === selectedId) ?? null;
  const n = {
    all: known.length,
    unused: known.filter((i) => i.status.state === "unused").length,
    built_in: known.filter((i) => facts(i)?.origin === "built_in").length,
    skill: known.filter((i) => facts(i)?.skill).length,
  };
  const defined = known.filter((i) => facts(i)?.origin === "file").length;
  const total = all.reduce((x, i) => x + runs(i), 0);
  const { sort, toggle: onSort } = useSort<"name" | "agents" | "runs" | "last">();
  const ordered = useMemo(
    () =>
      known
        .filter((i) =>
          filter === "all" ? true : filter === "unused" ? i.status.state === "unused" : filter === "skill" ? facts(i)?.skill : facts(i)?.origin === filter,
        )
        .filter((i) => !agent || i.agents.includes(agent))
        .filter((i) => !query || i.name.toLowerCase().includes(query.trim().toLowerCase()))
        .sort((a, b) => runs(b) - runs(a) || a.name.localeCompare(b.name)),
    [known, filter, agent, query],
  );
  const rows = sortBy(ordered, sort, (i, k) =>
    k === "name" ? i.name : k === "agents" ? i.agents.length : k === "runs" ? (i.usage ? runs(i) : null) : (i.usage?.last_used ?? null),
  );
  const chip = (k: Filter, label: string) =>
    n[k] || k === "all" ? <FChip label={label} count={n[k]} active={filter === k} onClick={() => setFilter(filter === k && k !== "all" ? "all" : k)} /> : null;
  const setTab = (next: Tab) => {
    setSelectedId(null);
    setParams({ ...(next === "command" ? { tab: "command" } : {}), ...(agent ? { agent } : {}) });
  };
  const prefix = tab === "command" ? "/" : "";

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-6 overflow-auto page">
        <PageHeader
          title={t.subagentsTitle}
          search={
            <SearchField
              value={query}
              onChange={setQuery}
              label={tab === "subagent" ? t.searchSubagents : t.searchCommands}
              count={query ? t.nResults(rows.length) : null}
              className="w-64"
            />
          }
        />
        <div role="tablist" className="flex gap-1 self-start rounded-full bg-panel p-1">
          {(["subagent", "command"] as Tab[]).map((k) => (
            <button
              key={k}
              type="button"
              role="tab"
              aria-selected={tab === k}
              onClick={() => setTab(k)}
              className={[
                "h-8 rounded-full px-4 text-[13px] font-semibold active:scale-[0.97]",
                tab === k ? "bg-ink text-bg" : "text-ink-2 hover:bg-sunk",
              ].join(" ")}
            >
              {k === "subagent" ? t.subagents : t.commands}
              <span className="tnum ml-1.5 font-normal opacity-70">
                {formatCount(((k === "subagent" ? subagents : commands).data ?? []).filter((i) => facts(i)?.origin !== "unknown").length)}
              </span>
            </button>
          ))}
        </div>
        {q.error ? (
          <p role="alert" className="text-danger">
            {getErrorMessage(q.error, t.loadFailed)}
          </p>
        ) : null}
        <StatTiles
          label={t.subagentsTitle}
          items={[
            {
              label: t.defined,
              value: formatCount(n.all),
              split: [
                { label: t.inFiles, value: defined, tone: "accent" },
                { label: t.builtIn, value: n.built_in, tone: "muted" },
                { label: t.fromSkill, value: known.filter((i) => facts(i)?.origin === "skill").length, tone: "ok" },
              ],
            },
            {
              label: t.runs,
              value: formatCount(total),
              bars: all.map((i) => ({ label: i.name, value: Number(i.usage?.count ?? 0), display: formatCount(Number(i.usage?.count ?? 0)) })),
            },
            {
              label: t.neverRun,
              value: formatCount(n.unused),
              hint: n.unused ? t.neverRunHint : t.allRan,
              tone: n.unused > 0 ? "warn" : null,
              onClick: () => setFilter(filter === "unused" ? "all" : "unused"),
              active: filter === "unused",
            },
          ]}
        />
        <div className="flex flex-wrap items-center gap-2">
          {chip("all", t.all)}
          {chip("unused", t.neverRun)}
          {chip("built_in", t.builtIn)}
          {chip("skill", t.fromSkill)}
          <AgentFilter agents={agents} items={known} value={agent} onChange={setAgent} />
        </div>
        {q.isPending ? null : (
          <table className="w-full table-fixed border-collapse text-left text-[14px]">
            <thead>
              <tr className="text-[12.5px] font-semibold text-ink-3">
                <Th table="subagents" label={t.colName} k="name" first="asc" sort={sort} onSort={onSort} className="pr-4" />
                <Th table="subagents" label={t.agentsCol} k="agents" sort={sort} onSort={onSort} className="w-[150px] pr-2" />
                <Th table="subagents" label={t.runs} k="runs" sort={sort} onSort={onSort} align="right" resize={!selected} className="w-20 pr-4" />
                {selected ? null : <Th table="subagents" label={t.lastRun} k="last" sort={sort} onSort={onSort} resize={false} className="w-28" />}
              </tr>
            </thead>
            <tbody>
              {rows.map((i) => (
                <Row
                  key={i.id}
                  item={i}
                  prefix={prefix}
                  columns={columns}
                  active={selected?.id === i.id}
                  compact={selected !== null}
                  onOpen={() => setSelectedId(i.id)}
                />
              ))}
            </tbody>
          </table>
        )}
        {unknown.length ? (
          <div>
            <h2 className="mb-1 mt-4 text-[13px] font-semibold text-ink-2">
              {t.noFileNow} <span className="tnum font-normal text-ink-3">{unknown.length}</span>
            </h2>
            <p className="mb-2 mt-0 text-[13px] text-ink-3">{t.noFileNowNote}</p>
            <ul className="m-0 flex list-none flex-wrap gap-2 p-0">
              {unknown.map((i) => (
                <li key={i.id} className="rounded-full bg-panel px-3 py-1 text-[13px]">
                  {prefix}
                  {i.name} <span className="tnum text-ink-3">{formatCount(runs(i))}</span>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
        <p className="m-0 text-[12.5px] text-ink-3">{t.runsNote}</p>
      </section>
      {selected && facts(selected)?.origin !== "unknown" ? (
        <SheetSteps ids={rows.filter((r) => facts(r)?.origin !== "unknown").map((r) => r.id)} current={selectedId} go={setSelectedId}>
          <DefSheet key={selected.id} item={selected} prefix={prefix} agents={agents} onClose={() => setSelectedId(null)} />
        </SheetSteps>
      ) : null}
    </div>
  );
}

function Badges({ item }: { item: Item }) {
  const f = facts(item)!;
  const pill = (label: string, tone: string) => <span className={["ml-2 rounded-full px-2 py-0.5 text-[11.5px] font-medium", tone].join(" ")}>{label}</span>;
  const plugin = f.locations.find((l) => l.plugin)?.plugin;
  return (
    <>
      {item.status.state === "unused" ? pill(t.neverRun, "bg-sunk text-warn-text") : null}
      {item.status.state === "off" ? pill(t.off, "bg-sunk text-ink-3") : null}
      {f.origin === "built_in" ? pill(t.builtIn, "bg-sunk text-ink-3") : null}
      {f.skill ? pill(t.fromSkill, "bg-accent-soft text-ink-2") : null}
      {plugin ? pill(plugin.id.split("@")[0] ?? plugin.id, "bg-sunk text-ink-3") : null}
    </>
  );
}

function Row({
  item,
  prefix,
  columns,
  active,
  compact,
  onOpen,
}: {
  item: Item;
  prefix: string;
  columns: AgentSummary[];
  active: boolean;
  compact: boolean;
  onOpen: () => void;
}) {
  const projects = new Set(
    facts(item)!
      .locations.map((l) => l.project)
      .filter(Boolean),
  );
  return (
    <tr data-row-id={item.id} onClick={onOpen} className={["cursor-default border-t border-rule-2", active ? "bg-accent-soft" : "hover:bg-sunk"].join(" ")}>
      <td className="truncate py-3 pr-4">
        <button type="button" onClick={onOpen} className="font-semibold tracking-[-0.02em]">
          {prefix}
          {item.name}
        </button>
        <Badges item={item} />
        {projects.size ? <span className="ml-2 text-[12.5px] text-ink-3">{[...projects].map((p) => tildePath(p!)).join(", ")}</span> : null}
      </td>
      <td className="py-3 pr-2">
        <AgentStack agents={item.agents} name={(k) => columns.find((a) => a.key === k)?.display_name ?? k} />
      </td>
      <td className="tnum py-3 pr-4 text-right">{item.usage ? formatCount(runs(item)) : "—"}</td>
      {compact ? null : <td className="tnum py-3 text-ink-2">{formatDate(item.usage?.last_used)}</td>}
    </tr>
  );
}

function DefSheet({ item, prefix, agents, onClose }: { item: Item; prefix: string; agents: AgentSummary[]; onClose: () => void }) {
  const f = facts(item)!;
  const navigate = useNavigate();
  const pluginRoute = usePluginRoute();
  const skillTo = useSkillRoute()(item.name, item.agents);
  const doc = useQuery({
    queryKey: ["doc", item.id],
    queryFn: () => ipc.itemDocument(item.id),
    enabled: f.locations.length > 0,
  });
  const md = doc.data && !f.locations[0]?.path.endsWith(".toml") ? stripFrontmatter(doc.data) : null;
  return (
    <DetailSheet title={item.name} onClose={onClose} width={480}>
      <h2 className="m-0 break-words text-[24px] font-bold leading-tight tracking-[-0.03em]">
        {prefix}
        {item.name}
        <Badges item={item} />
      </h2>
      {f.description ? <p className="mt-2 text-ink-2">{f.description}</p> : null}
      {f.model ? (
        <p className="mt-2 text-[13px] text-ink-3">
          {t.modelLabel} {f.model}
        </p>
      ) : null}
      {f.origin === "built_in" ? <p className="mt-4 text-[13px] text-ink-3">{t.builtInNote}</p> : null}
      {f.skill ? (
        <div className="mt-4">
          <p className="m-0 text-[13px] text-ink-3">{t.skillNote}</p>
          {skillTo ? (
            <button
              type="button"
              onClick={() => navigate(skillTo)}
              className="mt-2 h-8 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
            >
              {t.openSkill}
            </button>
          ) : null}
        </div>
      ) : null}
      {f.locations.length ? (
        <>
          <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">
            {t.definedIn} · {f.locations.length}
          </h3>
          <ul className="m-0 flex list-none flex-col gap-2 p-0">
            {f.locations.map((l) => (
              <li key={l.path} className="rounded-row bg-panel px-3 py-2.5">
                <div className="flex items-center gap-2">
                  <span className="font-semibold">{agentName(agents, l.agent)}</span>
                  <span className="truncate text-[12.5px] text-ink-3">
                    {l.project ? <PathLink path={l.project}>{tildePath(l.project)}</PathLink> : t.scopeUser}
                    {l.plugin ? (
                      <>
                        {` · ${t.plugin} `}
                        {pluginRoute(l.plugin.id) ? (
                          <PlaceLink onClick={() => navigate(pluginRoute(l.plugin!.id)!)} title={t.openPlugin}>
                            {l.plugin.id}
                          </PlaceLink>
                        ) : (
                          l.plugin.id
                        )}
                      </>
                    ) : null}
                    {l.name !== item.name ? ` · ${prefix}${l.name}` : ""}
                  </span>
                </div>
                <PathLink path={l.path} className="mt-1 block font-mono text-[11.5px] text-ink-3">
                  {tildePath(l.path)}
                </PathLink>
              </li>
            ))}
          </ul>
        </>
      ) : null}
      {f.locations.length ? (
        <article className="skill-md selectable mt-6 border-t border-rule pt-4 text-[14px]">
          {doc.error ? <p className="text-danger">{getErrorMessage(doc.error, t.loadFailed)}</p> : null}
          {md !== null ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{md}</ReactMarkdown> : null}
          {doc.data && md === null ? <pre className="whitespace-pre-wrap font-mono text-[12px]">{doc.data}</pre> : null}
        </article>
      ) : null}
    </DetailSheet>
  );
}
