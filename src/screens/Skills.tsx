import { PageHeader } from "../components/PageHeader";
import { useEffect, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { PathLink, PlaceLink, usePluginRoute } from "../components/PlaceLink";
import { useQuery } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { ipc } from "../ipc/client";
import { useItems, useOverview } from "../ipc/queries";
import { useAction } from "../actions/ActionProvider";
import { getErrorMessage } from "../ipc/error";
import type { Item } from "../ipc/bindings/Item";
import type { AgentSummary } from "../ipc/bindings/AgentSummary";
import type { SkillFacts } from "../ipc/bindings/SkillFacts";
import { AgentLogo, AgentStack } from "../components/AgentLogo";
import { SearchField } from "../components/SearchField";
import { Th, sortBy, useSort } from "../components/SortHeader";
import { Switch } from "../components/Switch";
import { InstallSheet } from "./InstallSheet";
import type { SkillLocation } from "../ipc/bindings/SkillLocation";
import { FChip } from "../components/FChip";
import { DetailSheet, SheetSteps } from "../components/DetailSheet";
import { agentName } from "../lib/agents";
import { formatCount, formatDate, stripFrontmatter, tildePath } from "../lib/format";
import { t } from "../strings";

type StatusFilter = "all" | "used" | "unused" | "gone" | "broken";

function facts(item: Item): SkillFacts | null {
  return item.facts.kind === "skill" ? item.facts : null;
}

const live = (l: SkillLocation) => l.link.kind !== "dangling";
/** Broken: the skill cannot load, or one of its links points nowhere (the Fix list counts these). */
const isBroken = (s: Item) => s.status.state === "broken" || (facts(s)?.locations.some((l) => l.link.kind === "dangling") ?? false);
/** Global real folders outside plugins and projects: the skill's own copies. */
const ownCopies = (f: SkillFacts) => f.locations.filter((l) => l.link.kind === "real_dir" && !l.plugin && !l.project);

/** Home label under the name (DESIGN.md §4). */
export function homeLabel(item: Item, agents: AgentSummary[]): string {
  const f = facts(item);
  if (!f || item.status.state === "gone") return "";
  const copies = ownCopies(f);
  if (copies.length > 1) return t.separateCopies(copies.length);
  if (copies.length === 1) {
    const home = copies[0]!;
    const folder = home.agents.includes("agents_shared") ? "~/.agents" : null;
    const linkers = f.locations.filter((l) => l.link.kind === "link" && live(l)).flatMap((l) => l.agents);
    const who = [...new Set(linkers)].map((a) => agentName(agents, a));
    if (who.length) return t.inFolderLinked(folder ?? agentName(agents, home.agents[0]!), who.join(", "));
    return folder ? t.inFolder(folder) : t.onlyIn(agentName(agents, home.agents[0]!));
  }
  const plugin = f.locations.find((l) => l.plugin)?.plugin;
  if (plugin) return t.fromPlugin(plugin.id);
  if (f.locations.some((l) => l.project && live(l))) return t.inProject;
  if (f.locations.length && f.locations.every((l) => !live(l))) return t.linksNowhere;
  return "";
}

export function Skills() {
  const { data, error, isPending } = useItems("skill");
  const overview = useOverview();
  const agents = overview.data?.agents ?? [];
  const [installing, setInstalling] = useState(false);
  const [params] = useSearchParams();
  const [status, setStatus] = useState<StatusFilter>((params.get("status") as StatusFilter) ?? "all");
  const [agent, setAgent] = useState<string | null>(params.get("agent"));
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(params.get("id"));
  // Deep links (Fix list "Show me", tray) may arrive while this screen is open.
  useEffect(() => {
    setStatus((params.get("status") as StatusFilter) ?? "all");
    setAgent(params.get("agent"));
    setSelectedId(params.get("id"));
  }, [params]);
  const open = (id: string | null) => {
    setInstalling(false);
    setSelectedId(id);
  };

  const skills = useMemo(() => (data ?? []).filter((i) => i.status.state !== "not_an_item"), [data]);
  const found = skills.find((s) => s.id === selectedId) ?? null;
  const selected = installing ? null : found;
  const compact = selected !== null || installing;
  const counts = useMemo(
    () => ({
      all: skills.filter((s) => s.status.state !== "gone").length,
      used: skills.filter((s) => (s.usage?.count ?? 0) > 0 && s.status.state !== "gone").length,
      unused: skills.filter((s) => s.status.state === "unused").length,
      gone: skills.filter((s) => s.status.state === "gone").length,
      broken: skills.filter(isBroken).length,
    }),
    [skills],
  );
  const filtered = skills.filter((s) => {
    const st = s.status.state;
    if (status === "all" && st === "gone") return false;
    if (status === "used" && ((s.usage?.count ?? 0) === 0 || st === "gone")) return false;
    if (status === "broken" && !isBroken(s)) return false;
    if ((status === "unused" || status === "gone") && st !== status) return false;
    if (agent && !s.agents.includes(agent)) return false;
    if (query && !`${s.name} ${facts(s)?.description ?? ""}`.toLowerCase().includes(query.toLowerCase())) return false;
    return true;
  });
  const { sort, toggle: onSort } = useSort<"name" | "description" | "agents" | "runs" | "last">();
  const rows = sortBy(filtered, sort, (s, k) =>
    k === "name"
      ? s.name
      : k === "description"
        ? (facts(s)?.description ?? null)
        : k === "agents"
          ? s.agents.length
          : k === "runs"
            ? s.usage
              ? Number(s.usage.count)
              : 0
            : (s.usage?.last_used ?? null),
  );

  const chip = (k: StatusFilter, label: string) => (
    <FChip label={label} count={counts[k]} active={status === k} onClick={() => setStatus(status === k && k !== "all" ? "all" : k)} />
  );

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-5 overflow-auto page">
        <PageHeader
          title={t.skills}
          search={<SearchField value={query} onChange={setQuery} label={t.search} count={query ? t.nResults(rows.length) : null} className="w-64" />}
        >
          <button
            type="button"
            onClick={() => {
              setSelectedId(null);
              setInstalling(true);
            }}
            className="h-10 rounded-full bg-ink px-5 text-[14px] font-semibold text-bg active:scale-[0.97]"
          >
            {t.installSkill}
          </button>
        </PageHeader>
        <div className="flex flex-wrap items-center gap-2">
          {chip("all", t.all)}
          {chip("used", t.used)}
          {chip("unused", t.unused)}
          {chip("gone", t.gone)}
          {chip("broken", t.broken)}
          <span className="mx-1 h-5 w-px bg-rule" />
          {agents
            .filter((a) => a.tier !== "skills")
            .map((a) => (
              <FChip
                key={a.key}
                icon={<AgentLogo agent={a} size={14} />}
                label={a.key === "agents_shared" ? t.sharedAgents : a.display_name}
                count={skills.filter((s) => s.status.state !== "gone" && s.agents.includes(a.key)).length}
                active={agent === a.key}
                onClick={() => setAgent(agent === a.key ? null : a.key)}
              />
            ))}
        </div>
        {error ? (
          <p role="alert" className="text-danger">
            {getErrorMessage(error, t.loadFailed)}
          </p>
        ) : null}
        {isPending ? null : (
          <table className="w-full table-fixed border-collapse text-left text-[14px]">
            <thead>
              <tr className="text-[12.5px] font-semibold text-ink-3">
                <Th table="skills" label={t.colName} k="name" first="asc" sort={sort} onSort={onSort} className={compact ? "pr-4" : "w-[30%] pr-4"} />
                {compact ? null : <Th table="skills" label={t.colDescription} k="description" first="asc" sort={sort} onSort={onSort} className="pr-4" />}
                <Th table="skills" label={t.agentsCol} k="agents" sort={sort} onSort={onSort} className="w-[150px] pr-2" />
                <Th table="skills" label={t.colRuns} k="runs" sort={sort} onSort={onSort} align="right" className="w-20 pr-4" />
                <Th table="skills" label={t.colLastUsed} k="last" sort={sort} onSort={onSort} resize={false} className="w-28" />
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr>
                  <td colSpan={compact ? 4 : 5} className="py-10 text-center text-[13px] text-ink-3">
                    {t.noFilterMatches}
                  </td>
                </tr>
              ) : null}
              {rows.map((s) => (
                <SkillRow
                  key={s.id}
                  item={s}
                  agentName={(k) => agentName(agents, k)}
                  active={selected?.id === s.id}
                  compact={compact}
                  label={homeLabel(s, agents)}
                  onOpen={() => open(s.id)}
                />
              ))}
            </tbody>
          </table>
        )}
      </section>
      {selected ? (
        <SheetSteps ids={rows.map((r) => r.id)} current={selectedId} go={setSelectedId}>
          <SkillSheet key={selected.id} item={selected} agents={agents} onClose={() => setSelectedId(null)} />
        </SheetSteps>
      ) : null}
      {installing ? (
        <InstallSheet
          onClose={() => setInstalling(false)}
          onInstalled={(name) => {
            setInstalling(false);
            setQuery(name);
          }}
        />
      ) : null}
    </div>
  );
}

function SkillRow({
  item,
  agentName,
  active,
  compact,
  label,
  onOpen,
}: {
  item: Item;
  agentName: (key: string) => string;
  active: boolean;
  compact: boolean;
  label: string;
  onOpen: () => void;
}) {
  const f = facts(item);
  const st = item.status.state;
  return (
    <tr data-row-id={item.id} onClick={onOpen} className={["cursor-default border-t border-rule-2", active ? "bg-accent-soft" : "hover:bg-sunk"].join(" ")}>
      <td className="truncate py-2.5 pr-4">
        <button type="button" onClick={onOpen} className="font-semibold tracking-[-0.02em] text-ink">
          {item.name}
        </button>
        {st !== "broken" && isBroken(item) ? (
          <span className="ml-2 rounded-full bg-danger-soft px-2 py-0.5 text-[11.5px] font-medium text-danger">{t.status.broken}</span>
        ) : null}
        {st === "unused" || st === "gone" || st === "broken" || st === "off" ? (
          <span
            className={[
              "ml-2 rounded-full px-2 py-0.5 text-[11.5px] font-medium",
              st === "broken" ? "bg-danger-soft text-danger" : st === "unused" ? "bg-sunk text-warn-text" : "bg-sunk text-ink-3",
            ].join(" ")}
          >
            {t.status[st]}
          </span>
        ) : null}
        {label ? (
          <div className="mt-0.5 flex items-center gap-2 truncate text-[12px]">
            <span className="text-ink-3">{label}</span>
          </div>
        ) : null}
      </td>
      {compact ? null : <td className="truncate py-3 pr-4 text-ink-2">{f?.description ?? ""}</td>}
      <td className="py-3 pr-2">
        {item.agents.length ? <AgentStack agents={item.agents} name={agentName} /> : <span className="text-[12.5px] text-ink-3">—</span>}
      </td>
      <td className="tnum py-3 pr-4 text-right">{item.usage ? formatCount(item.usage.count) : st === "gone" ? "" : "0"}</td>
      <td className="tnum py-3 text-ink-2">{formatDate(item.usage?.last_used)}</td>
    </tr>
  );
}

function SkillSheet({ item, agents, onClose }: { item: Item; agents: AgentSummary[]; onClose: () => void }) {
  const f = facts(item);
  const { run } = useAction();
  const hasFile = (f?.locations ?? []).some((l) => l.link.kind !== "dangling");
  const doc = useQuery({
    queryKey: ["doc", item.id],
    queryFn: () => ipc.itemDocument(item.id),
    enabled: hasFile,
  });
  const targets = useQuery({
    queryKey: ["skill-targets", item.id, item.agents.join(",")],
    queryFn: () => ipc.skillTargets(item.id),
    enabled: hasFile && item.status.state !== "gone",
  });
  const name = (key: string) => agentName(agents, key);
  const navigate = useNavigate();
  const pluginRoute = usePluginRoute();
  // The switch moves on press; the scan after the write confirms it. A failed
  // or cancelled write falls back to what is on disk.
  const [want, setWant] = useState<Record<string, boolean>>({});
  const agentsKey = item.agents.join(",");
  useEffect(() => setWant({}), [agentsKey]);
  const toggle = async (agent: string, on: boolean) => {
    setWant((w) => ({ ...w, [agent]: on }));
    try {
      await run({ type: "skill_toggle", id: item.id, agent, on });
    } finally {
      window.setTimeout(
        () =>
          setWant((w) => {
            const { [agent]: _, ...rest } = w;
            return rest;
          }),
        1500,
      );
    }
  };
  const tgs = targets.data ?? [];
  // Same meaning as the row's dots: the agent sees the skill (from any folder it reads).
  // Groups use the state on disk, so a row does not jump between lists mid-press.
  const isOn = (key: string) => item.agents.includes(key);
  const onFor = tgs.filter((x) => isOn(x.key));
  const notOn = tgs.filter((x) => !isOn(x.key));
  const targetRow = (x: (typeof tgs)[number]) => {
    const pending = x.key in want;
    const on = pending ? want[x.key]! : isOn(x.key);
    const others = x.agents.filter((a) => a !== x.key && a !== "agents_shared");
    return (
      <li key={x.key} className="flex items-center gap-3 rounded-row bg-panel px-3 py-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 font-semibold">
            <AgentLogo agent={x.key} size={16} />
            {name(x.key)}
          </div>
          <div className="truncate text-[12px] text-ink-3" title={x.dir}>
            {others.length ? `${t.readBy(others.map(name).join(", "))} · ` : ""}
            {tildePath(x.dir)}
          </div>
        </div>
        <Switch on={on} disabled={pending} label={`${name(x.key)}: ${on ? t.on : t.off}`} onChange={(next) => void toggle(x.key, next)} />
      </li>
    );
  };
  return (
    <DetailSheet title={item.name} onClose={onClose}>
      <h2 className="m-0 text-[24px] font-bold leading-tight tracking-[-0.03em]">{item.name}</h2>
      {f?.description ? <p className="mt-2 text-ink-2">{f.description}</p> : null}
      {tgs.length ? (
        <>
          <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">
            {t.linkedIn} · {onFor.length}
          </h3>
          <p className="m-0 mb-2 text-[12.5px] leading-relaxed text-ink-3">{t.skillSwitchNote}</p>
          <ul className="m-0 flex list-none flex-col gap-2 p-0">{onFor.map(targetRow)}</ul>
          {notOn.length ? (
            <>
              <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">
                {t.notLinked} · {notOn.length}
              </h3>
              <ul className="m-0 flex list-none flex-col gap-2 p-0">{notOn.map(targetRow)}</ul>
            </>
          ) : null}
        </>
      ) : null}
      {f && f.locations.length ? (
        <>
          <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">
            {t.places} · {f.locations.length}
          </h3>
          <ul className="m-0 flex list-none flex-col gap-2 p-0">
            {f.locations.map((l) => (
              <li key={l.path} data-row-id={l.link.kind === "dangling" ? l.path : undefined} className="flex items-center gap-3 rounded-row bg-panel px-3 py-2">
                <span className="inline-flex shrink-0 -space-x-1">
                  {l.agents.slice(0, 3).map((a) => (
                    <AgentLogo key={a} agent={a} size={18} />
                  ))}
                </span>
                <div className="min-w-0 flex-1">
                  {l.link.kind === "dangling" ? (
                    <div className="selectable truncate font-mono text-[12.5px]" title={l.path}>
                      {tildePath(l.path)}
                    </div>
                  ) : (
                    <PathLink path={l.path} className="block font-mono text-[12.5px]">
                      {tildePath(l.path)}
                    </PathLink>
                  )}
                  <div className="mt-0.5 truncate text-[12.5px] text-ink-2">
                    {l.agents.map(name).join(", ")}
                    {l.link.kind === "link" ? (
                      <>
                        {` · ${t.linkTo} `}
                        <PathLink path={l.link.target}>{tildePath(l.link.target)}</PathLink>
                      </>
                    ) : null}
                    {l.link.kind === "dangling" ? <span className="text-danger"> · {t.dangling}</span> : null}
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
                        {l.plugin.enabled ? "" : ` (${t.status.off})`}
                      </>
                    ) : null}
                    {l.project ? (
                      <>
                        {` · ${t.project} `}
                        <PathLink path={l.project}>{tildePath(l.project)}</PathLink>
                      </>
                    ) : null}
                  </div>
                </div>
                {l.link.kind === "dangling" ? (
                  <button
                    type="button"
                    title={t.removeLinkNote}
                    onClick={() => void run({ type: "link_remove", id: item.id, path: l.path })}
                    className="h-7 shrink-0 rounded-full px-3 text-[12.5px] font-semibold text-danger ring-1 ring-rule hover:bg-danger-soft active:scale-[0.96]"
                  >
                    {t.removeLink}
                  </button>
                ) : null}
              </li>
            ))}
          </ul>
          {f.locations.filter((l) => l.link.kind === "dangling").length > 1 ? (
            <button
              type="button"
              onClick={() => void run({ type: "links_remove_dangling", ids: [item.id] })}
              className="mt-3 h-9 rounded-full px-4 text-[13px] font-semibold text-danger ring-1 ring-rule hover:bg-danger-soft active:scale-[0.97]"
            >
              {t.removeAllLinks(f.locations.filter((l) => l.link.kind === "dangling").length)}
            </button>
          ) : null}
        </>
      ) : null}
      {hasFile ? (
        <article className="skill-md selectable mt-6 border-t border-rule pt-4 text-[14px]">
          {doc.error ? <p className="text-danger">{getErrorMessage(doc.error, t.loadFailed)}</p> : null}
          {doc.data ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{stripFrontmatter(doc.data)}</ReactMarkdown> : null}
        </article>
      ) : null}
    </DetailSheet>
  );
}
