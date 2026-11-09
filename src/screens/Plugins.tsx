import { SearchField } from "../components/SearchField";
import { StatTiles } from "../components/StatTiles";
import { PageHeader } from "../components/PageHeader";
import { AgentLogo } from "../components/AgentLogo";
import { Th, sortBy, useSort, type Sort } from "../components/SortHeader";
import { agentName } from "../lib/agents";
import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useItems, useOverview } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import { useAction } from "../actions/ActionProvider";
import type { Item } from "../ipc/bindings/Item";
import type { PluginFacts } from "../ipc/bindings/PluginFacts";
import type { MarketplaceFacts } from "../ipc/bindings/MarketplaceFacts";
import type { CacheFacts } from "../ipc/bindings/CacheFacts";
import { FChip } from "../components/FChip";
import { Switch } from "../components/Switch";
import { DetailSheet, SheetSteps } from "../components/DetailSheet";
import { formatCount, formatDate, formatBytes, tildePath } from "../lib/format";
import { t } from "../strings";

type View = "plugins" | "marketplaces" | "cache";
type StatusFilter = "all" | "ok" | "off" | "gone";

const pf = (i: Item): PluginFacts | null => (i.facts.kind === "plugin" ? i.facts : null);
const mf = (i: Item): MarketplaceFacts | null => (i.facts.kind === "marketplace" ? i.facts : null);
const cf = (i: Item): CacheFacts | null => (i.facts.kind === "plugin_cache" ? i.facts : null);

export function Plugins() {
  const [params] = useSearchParams();
  const [view, setView] = useState<View>((params.get("view") as View) ?? "plugins");
  const [status, setStatus] = useState<StatusFilter>((params.get("status") as StatusFilter) ?? "all");
  const [selectedId, setSelectedId] = useState<string | null>(params.get("id"));
  useEffect(() => {
    setView((params.get("view") as View) ?? "plugins");
    setStatus((params.get("status") as StatusFilter) ?? "all");
    setSelectedId(params.get("id"));
  }, [params]);

  const plugins = useItems("plugin");
  const agentList = useOverview().data?.agents ?? [];
  const markets = useItems("marketplace");
  const cache = useItems("plugin_cache");
  const error = plugins.error ?? markets.error ?? cache.error;
  const selected = (plugins.data ?? []).find((p) => p.id === selectedId) ?? null;
  const { sort, toggle: onSort } = useSort<PluginKey>();
  const [query, setQuery] = useState("");
  const [agent, setAgent] = useState<string | null>(null);
  const shownPlugins = (plugins.data ?? []).filter((p) => matches(p, query) && (!agent || p.agents.includes(agent)));

  const unusedMarkets = (markets.data ?? []).filter((m) => m.status.state === "unused");
  const cacheBytes = (cache.data ?? []).reduce((n, c) => n + Number(cf(c)?.size_bytes ?? 0), 0);

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-6 overflow-auto page">
        <PageHeader
          title={t.plugins}
          lead={t.pluginsLead}
          search={<SearchField value={query} onChange={setQuery} label={t.searchPlugins} className="w-64" />}
        />
        {error ? (
          <p role="alert" className="text-danger">
            {getErrorMessage(error, t.loadFailed)}
          </p>
        ) : null}
        <StatTiles
          label={t.plugins}
          items={[
            {
              label: t.installed,
              value: formatCount((plugins.data ?? []).filter((p) => pf(p)?.installed).length),
              bars: agentList.map((a) => ({
                label: a.display_name,
                value: (plugins.data ?? []).filter((p) => pf(p)?.installed && p.agents.includes(a.key)).length,
                icon: <AgentLogo agent={a.key} size={13} />,
              })),
              onClick: () => (setView("plugins"), setStatus("all")),
            },
            {
              label: t.ghosts,
              value: formatCount((plugins.data ?? []).filter((p) => p.status.state === "gone").length),
              hint: t.ghostsHint,
              tone: (plugins.data ?? []).some((p) => p.status.state === "gone") ? "warn" : null,
              onClick: () => (setView("plugins"), setStatus(status === "gone" ? "all" : "gone")),
              active: view === "plugins" && status === "gone",
            },
            {
              label: t.marketplaces,
              value: formatCount(markets.data?.length ?? 0),
              hint: unusedMarkets.length ? t.supplyNothing(unusedMarkets.length) : t.allInUse,
              onClick: () => setView("marketplaces"),
              active: view === "marketplaces",
            },
            {
              label: t.oldCache,
              value: formatBytes(cacheBytes),
              hint: t.oldVersions(cache.data?.length ?? 0),
              onClick: () => setView("cache"),
              active: view === "cache",
            },
          ]}
        />
        <div role="radiogroup" aria-label={t.view} className="inline-flex h-8 w-fit items-center rounded-full bg-sunk p-0.5 text-[12.5px] font-medium">
          {(
            [
              ["plugins", t.plugins, plugins.data?.length ?? 0],
              ["marketplaces", t.marketplaces, markets.data?.length ?? 0],
              ["cache", t.cache, cache.data?.length ?? 0],
            ] as const
          ).map(([v, label, n]) => (
            <button
              key={v}
              type="button"
              role="radio"
              aria-checked={view === v}
              onClick={() => setView(v)}
              className={["h-7 rounded-full px-3", view === v ? "bg-bg text-ink shadow-card" : "text-ink-2"].join(" ")}
            >
              {label} <span className="tnum text-ink-3">{n}</span>
            </button>
          ))}
        </div>
        {view === "plugins" ? (
          <PluginList
            items={shownPlugins}
            all={(plugins.data ?? []).filter((p) => matches(p, query))}
            agent={agent}
            setAgent={setAgent}
            status={status}
            setStatus={setStatus}
            sort={sort}
            onSort={onSort}
            selectedId={selectedId}
            onOpen={setSelectedId}
            compact={selected !== null}
          />
        ) : view === "marketplaces" ? (
          <MarketList items={markets.data ?? []} unused={unusedMarkets} />
        ) : (
          <CacheList items={cache.data ?? []} bytes={cacheBytes} />
        )}
      </section>
      {selected && view === "plugins" ? (
        <SheetSteps ids={pluginOrder(shownPlugins, status, sort).map((p) => p.id)} current={selectedId} go={setSelectedId}>
          <PluginSheet key={selected.id} item={selected} onClose={() => setSelectedId(null)} />
        </SheetSteps>
      ) : null}
    </div>
  );
}

function Chip({ state }: { state: string }) {
  if (state === "ok") return null;
  const tone =
    state === "broken"
      ? "bg-danger-soft text-danger"
      : state === "unused"
        ? "bg-sunk text-warn-text"
        : state === "gone"
          ? "bg-sunk text-ink-3"
          : "bg-sunk text-ink-3";
  return (
    <span className={["ml-2 rounded-full px-2 py-0.5 text-[11.5px] font-medium", tone].join(" ")}>{state === "gone" ? t.uninstalled : t.status[state]}</span>
  );
}

/** On/off switch (plugins have a real switch in Claude settings). */
function contentsLine(f: PluginFacts): string {
  const c = f.contents;
  const parts: string[] = [];
  const add = (n: number, one: string, many: string) => n && parts.push(`${n} ${n === 1 ? one : many}`);
  add(c.skills, "skill", "skills");
  add(c.commands, "command", "commands");
  add(c.agents, "agent", "agents");
  add(c.hooks, "hook", "hooks");
  add(c.mcp_servers, "MCP server", "MCP servers");
  return parts.join(" · ");
}

function PluginList({
  items,
  all,
  agent,
  setAgent,
  status,
  setStatus,
  sort,
  onSort,
  selectedId,
  onOpen,
  compact,
}: {
  items: Item[];
  /** Before the agent filter, for its counts. */
  all: Item[];
  agent: string | null;
  setAgent: (a: string | null) => void;
  status: StatusFilter;
  setStatus: (s: StatusFilter) => void;
  sort: PluginSort;
  onSort: (k: PluginKey, first: "asc" | "desc") => void;
  selectedId: string | null;
  onOpen: (id: string) => void;
  compact: boolean;
}) {
  const { run } = useAction();
  const count = (st: string) => items.filter((i) => i.status.state === st).length;
  const rows = useMemo(() => pluginOrder(items, status, sort), [items, status, sort]);
  const agents = useOverview().data?.agents ?? [];
  // Agents with plugins, most plugins first.
  const byAgent = useMemo(() => {
    const n = new Map<string, number>();
    for (const p of all) for (const a of p.agents.slice(0, 1)) n.set(a, (n.get(a) ?? 0) + 1);
    return [...n].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }, [all]);
  const chip = (k: StatusFilter, label: string, n: number) => (
    <FChip label={label} count={n} active={status === k} onClick={() => setStatus(status === k && k !== "all" ? "all" : k)} />
  );
  return (
    <>
      <div className="flex flex-wrap items-center gap-2">
        {chip("all", t.all, items.length)}
        {chip("ok", t.on, count("ok"))}
        {chip("off", t.off, count("off"))}
        {chip("gone", t.uninstalled, count("gone"))}
        {byAgent.length > 1 ? <span className="mx-1 h-5 w-px bg-rule" /> : null}
        {byAgent.length > 1
          ? byAgent.map(([k, n]) => (
              <FChip
                key={k}
                label={agentName(agents, k)}
                icon={<AgentLogo agent={k} size={14} />}
                count={n}
                active={agent === k}
                onClick={() => setAgent(agent === k ? null : k)}
              />
            ))
          : null}
      </div>
      <table className="w-full table-fixed border-collapse text-left text-[14px]">
        <thead>
          <tr className="text-[12.5px] font-semibold text-ink-3">
            <Th table="plugins" label={t.colName} k="name" first="asc" sort={sort} onSort={onSort} className="pr-4" />
            {compact ? null : <Th table="plugins" label={t.colAgent} k="agent" first="asc" sort={sort} onSort={onSort} className="w-[150px] pr-4" />}
            {compact ? null : <Th table="plugins" label={t.contents} k="contents" first="asc" sort={sort} onSort={onSort} className="w-[28%] pr-4" />}
            <Th table="plugins" label={t.uses} k="uses" sort={sort} onSort={onSort} align="right" className="w-20 pr-4" />
            {compact ? null : <Th table="plugins" label={t.colLastUsed} k="last" sort={sort} onSort={onSort} className="w-28 pr-4" />}
            <Th table="plugins" label={t.onOff} k="on" sort={sort} onSort={onSort} align="right" resize={false} className="w-24" />
          </tr>
        </thead>
        <tbody>
          {rows.map((p) => {
            const f = pf(p)!;
            const st = p.status.state;
            return (
              <tr
                key={p.id}
                data-row-id={p.id}
                onClick={() => onOpen(p.id)}
                className={["cursor-default border-t border-rule-2", selectedId === p.id ? "bg-accent-soft" : "hover:bg-sunk"].join(" ")}
              >
                <td className="truncate py-3 pr-4">
                  <span className="font-semibold tracking-[-0.02em]">{p.name}</span>
                  <Chip state={st} />
                  <div className="truncate text-[12.5px] text-ink-3">
                    {f.marketplace}
                    {f.project ? ` · ${t.project} ${tildePath(f.project)}` : ""}
                  </div>
                </td>
                {compact ? null : (
                  <td className="py-3 pr-4">
                    <span className="inline-flex min-w-0 items-center gap-2 text-ink-2">
                      {p.agents.slice(0, 1).map((a) => (
                        <AgentLogo key={a} agent={a} size={16} />
                      ))}
                      <span className="truncate">{p.agents.map((a) => agentName(agents, a)).join(", ")}</span>
                    </span>
                  </td>
                )}
                {compact ? null : <td className="truncate py-3 pr-4 text-ink-2">{contentsLine(f)}</td>}
                <td className="tnum py-3 pr-4 text-right">
                  {p.usage ? (
                    formatCount(Number(p.usage.count))
                  ) : isClaude(p) ? (
                    "0"
                  ) : (
                    <span className="text-ink-3" title={t.noUsageData}>
                      —
                    </span>
                  )}
                </td>
                {compact ? null : <td className="tnum py-3 pr-4 text-ink-2">{formatDate(p.usage?.last_used)}</td>}
                <td className="py-3 text-right">
                  {canSwitch(p) && !isClaude(p) ? (
                    <span className="inline-flex justify-end">
                      <Switch
                        on={f.enabled !== false}
                        label={`${p.name}: ${f.enabled !== false ? t.on : t.off}`}
                        onChange={(on) => void run({ type: "plugin_set_enabled", id: p.id, on })}
                      />
                    </span>
                  ) : !isClaude(p) ? (
                    <span className="text-[12.5px] text-ink-3" title={t.switchIn(agentName(agents, p.agents[0] ?? ""))}>
                      {f.enabled === false ? t.off : f.enabled === true ? t.on : t.installed}
                    </span>
                  ) : f.installed && !f.project ? (
                    <span className="inline-flex justify-end">
                      <Switch
                        on={st === "ok"}
                        label={`${p.name}: ${st === "ok" ? t.on : t.off}`}
                        onChange={(on) => void run({ type: "plugin_set_enabled", id: p.id, on })}
                      />
                    </span>
                  ) : null}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </>
  );
}

/**
 * How to bring a removed plugin back: the steps the core worked out (DESIGN.md
 * "Restore"), numbered, with where the marketplace source came from. With no
 * known source there is nothing that would work to copy, so it says so.
 */
function Restore({ f, copied, onCopy }: { f: PluginFacts; copied: boolean; onCopy: () => void }) {
  const r = f.restore;
  if (!r) return null;
  if (!r.steps.length) {
    return <p className="mt-4 rounded-row bg-sunk px-3 py-2.5 text-[13px] leading-relaxed text-warn-text">{t.noMarketSource(f.marketplace, f.id)}</p>;
  }
  return (
    <section className="mt-4 rounded-card bg-panel p-4 ring-1 ring-rule">
      <div className="flex items-center gap-3">
        <h3 className="m-0 text-[13px] font-semibold">{t.restoreSteps}</h3>
        <button type="button" onClick={onCopy} className="ml-auto h-8 rounded-full bg-ink px-3.5 text-[12.5px] font-semibold text-bg active:scale-[0.97]">
          <span aria-live="polite">{copied ? t.copied : r.steps.length > 1 ? t.copyInstallSteps : t.copyInstall}</span>
        </button>
      </div>
      <ol className="selectable m-0 mt-3 flex list-none flex-col gap-1.5 p-0">
        {r.steps.map((s, i) => (
          <li key={s} className="flex items-baseline gap-2.5">
            <span className="tnum w-4 shrink-0 text-right text-[12px] text-ink-3">{i + 1}</span>
            <code className="min-w-0 break-all rounded-input bg-bg px-2 py-1 font-mono text-[12.5px] ring-1 ring-rule">{s}</code>
          </li>
        ))}
      </ol>
      {r.source && r.origin ? <p className="mb-0 mt-3 text-[12.5px] text-ink-3">{t.sourceFrom[r.origin](r.source)}</p> : null}
    </section>
  );
}

function PluginSheet({ item, onClose }: { item: Item; onClose: () => void }) {
  const f = pf(item)!;
  const { run } = useAction();
  const agentList = useOverview().data?.agents ?? [];
  const [copied, setCopied] = useState(false);
  return (
    <DetailSheet title={item.name} onClose={onClose} width={480}>
      <h2 className="m-0 text-[24px] font-bold leading-tight tracking-[-0.03em]">
        {item.name}
        <Chip state={item.status.state} />
      </h2>
      <p className="mt-2 text-ink-2">
        {f.marketplace}
        {f.version ? ` · ${f.version}` : ""}
        {f.size_bytes ? ` · ${formatBytes(Number(f.size_bytes))}` : ""}
      </p>
      {canSwitch(item) && !isClaude(item) ? (
        <div className="mt-4 flex items-center gap-3">
          <Switch
            on={f.enabled !== false}
            label={`${item.name}: ${f.enabled !== false ? t.on : t.off}`}
            onChange={(on) => void run({ type: "plugin_set_enabled", id: item.id, on })}
          />
          <span className="text-[13px] text-ink-2">{t.switchFor(f.enabled !== false, agentName(agentList, item.agents[0] ?? ""))}</span>
        </div>
      ) : !isClaude(item) ? (
        <p className="mt-3 flex items-center gap-2 rounded-row bg-sunk px-3 py-2 text-[13px] text-ink-2">
          <AgentLogo agent={item.agents[0] ?? ""} size={16} />
          {t.readOnlyPlugin(agentName(agentList, item.agents[0] ?? ""))}
        </p>
      ) : null}
      <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-3">
        <div hidden={!isClaude(item) && !item.usage}>
          <dt className="text-[12.5px] font-semibold text-ink-3">{t.uses}</dt>
          <dd className="tnum m-0 text-[22px] font-bold">{formatCount(Number(item.usage?.count ?? 0))}</dd>
        </div>
        {item.usage?.last_used ? (
          <div>
            <dt className="text-[12.5px] font-semibold text-ink-3">{t.colLastUsed}</dt>
            <dd className="m-0 text-[22px] font-bold">{formatDate(item.usage.last_used)}</dd>
          </div>
        ) : null}
      </dl>
      {f.installed ? (
        <>
          {contentsLine(f) ? <p className="mt-4 text-ink-2">{contentsLine(f)}</p> : null}
          {f.install_path ? (
            <div className="selectable mt-4 break-all rounded-row bg-panel px-3 py-2 font-mono text-[12.5px]">{tildePath(f.install_path)}</div>
          ) : null}
          <p className="mt-3 text-[13px] text-ink-3">
            {t.installedOn} {formatDate(f.installed_at)}
            {f.updated_at ? ` · ${t.updatedOn} ${formatDate(f.updated_at)}` : ""}
          </p>
        </>
      ) : (
        <>
          <p className="mt-4 text-ink-2">{t.ghostNote}</p>
          <Restore f={f} copied={copied} onCopy={() => void navigator.clipboard.writeText(f.restore?.steps.join("\n") ?? "").then(() => setCopied(true))} />
          {f.enabled !== null ? (
            <button
              type="button"
              onClick={() => void run({ type: "plugin_forget", id: item.id })}
              className="mt-3 h-10 rounded-full bg-bg px-5 font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
            >
              {t.forget}
            </button>
          ) : null}
        </>
      )}
    </DetailSheet>
  );
}

function MarketList({ items, unused }: { items: Item[]; unused: Item[] }) {
  const { run } = useAction();
  const removable = unused;
  const bytes = unused.reduce((n, m) => n + Number(mf(m)?.size_bytes ?? 0), 0);
  const { sort, toggle: onSort } = useSort<"name" | "supplies" | "size" | "updated">();
  const base = [...items].sort((a, b) => (a.status.state === b.status.state ? a.name.localeCompare(b.name) : a.status.state === "unused" ? -1 : 1));
  const rows = sortBy(base, sort, (m, k) => {
    const f = mf(m);
    return k === "name" ? m.name : k === "supplies" ? (f?.plugins.length ?? 0) : k === "size" ? Number(f?.size_bytes ?? 0) : (f?.updated_at ?? null);
  });
  return (
    <>
      {removable.length ? (
        <div className="flex items-center gap-4 rounded-card bg-panel px-5 py-4 shadow-card">
          <p className="m-0 flex-1 text-ink-2">{t.unusedMarkets(removable.length, formatBytes(bytes))}</p>
          <button
            type="button"
            onClick={() =>
              void run({
                type: "marketplace_remove",
                ids: removable.map((m) => m.id),
              })
            }
            className="h-10 rounded-full bg-ink px-5 font-semibold text-bg active:scale-[0.97]"
          >
            {t.removeN(removable.length)}
          </button>
        </div>
      ) : null}
      <table className="w-full table-fixed border-collapse text-left text-[14px]">
        <thead>
          <tr className="text-[12.5px] font-semibold text-ink-3">
            <Th table="marketplaces" label={t.colName} k="name" first="asc" sort={sort} onSort={onSort} className="pr-4" />
            <Th table="marketplaces" label={t.supplies} k="supplies" sort={sort} onSort={onSort} className="w-[30%] pr-4" />
            <Th table="marketplaces" label={t.size} k="size" sort={sort} onSort={onSort} align="right" className="w-24 pr-4" />
            <Th table="marketplaces" label={t.updated} k="updated" sort={sort} onSort={onSort} className="w-28 pr-4" />
            <th className="w-24 py-2" />
          </tr>
        </thead>
        <tbody>
          {rows.map((m) => {
            const f = mf(m)!;
            return (
              <tr key={m.id} data-row-id={m.id} className="border-t border-rule-2 hover:bg-sunk">
                <td className="truncate py-3 pr-4">
                  <span className="font-semibold tracking-[-0.02em]">{m.name}</span>
                  <Chip state={m.status.state} />
                  <div className="selectable truncate text-[12.5px] text-ink-3">{tildePath(f.source)}</div>
                </td>
                <td className="truncate py-3 pr-4 text-ink-2">{f.plugins.map((p) => p.split("@")[0]).join(", ") || t.nothing}</td>
                <td className="tnum py-3 pr-4 text-right text-ink-2">{f.size_bytes ? formatBytes(Number(f.size_bytes)) : ""}</td>
                <td className="tnum py-3 pr-4 text-ink-2">{formatDate(f.updated_at)}</td>
                <td className="py-3 text-right">
                  {m.status.state === "unused" ? (
                    <button
                      type="button"
                      onClick={() => void run({ type: "marketplace_remove", ids: [m.id] })}
                      className="h-8 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
                    >
                      {t.remove}
                    </button>
                  ) : null}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </>
  );
}

function CacheList({ items, bytes }: { items: Item[]; bytes: number }) {
  const { run } = useAction();
  if (!items.length) return <p className="text-ink-2">{t.cacheEmpty}</p>;
  return (
    <>
      <div className="flex items-center gap-4 rounded-card bg-panel px-5 py-4 shadow-card">
        <p className="m-0 flex-1 text-ink-2">{t.cacheNote(items.length, formatBytes(bytes))}</p>
        <button
          type="button"
          onClick={() => void run({ type: "cache_clean", ids: items.map((c) => c.id) })}
          className="h-10 rounded-full bg-ink px-5 font-semibold text-bg active:scale-[0.97]"
        >
          {t.cleanCache(formatBytes(bytes))}
        </button>
      </div>
      <ul className="m-0 flex list-none flex-col p-0">
        {items.map((c) => {
          const f = cf(c)!;
          return (
            <li key={c.id} data-row-id={c.id} className="flex items-center gap-4 border-t border-rule-2 py-3">
              <div className="min-w-0 flex-1">
                <div className="truncate font-semibold tracking-[-0.02em]">{c.name}</div>
                <div className="selectable truncate font-mono text-[12px] text-ink-3">{tildePath(f.path)}</div>
              </div>
              <span className="text-[13px] text-ink-3">{f.temp ? t.tempClone : t.oldVersion}</span>
              <span className="tnum w-20 text-right text-ink-2">{formatBytes(Number(f.size_bytes))}</span>
            </li>
          );
        })}
      </ul>
    </>
  );
}

type PluginKey = "name" | "agent" | "contents" | "uses" | "last" | "on";
type PluginSort = Sort<PluginKey>;

/** Rows as the list shows them; the sheet steps through the same order. */
function pluginOrder(items: Item[], status: StatusFilter, sort: PluginSort): Item[] {
  const base = items.filter((i) => status === "all" || i.status.state === status).sort((a, b) => Number(b.usage?.count ?? 0) - Number(a.usage?.count ?? 0));
  return sortBy(base, sort, (p, k) => {
    const f = pf(p);
    switch (k) {
      case "name":
        return p.name;
      case "agent":
        return p.agents[0] ?? null;
      case "contents":
        return f ? contentsLine(f) : null;
      case "uses":
        return Number(p.usage?.count ?? 0);
      case "last":
        return p.usage?.last_used ?? null;
      case "on":
        return f?.installed && !f.project ? (p.status.state === "ok" ? 1 : 0) : null;
    }
  });
}

const isClaude = (p: Item) => p.agents.includes("claude_code");
/**
 * Agents that keep the switch in a file: Gemini CLI (extension-enablement.json)
 * and OpenCode (its plugins folder and config list). Codex and Cursor keep it
 * in their own databases, which SkillDesk never writes.
 */
const canSwitch = (p: Item) => {
  const f = p.facts.kind === "plugin" ? p.facts : null;
  if (!f) return false;
  if (p.agents.includes("gemini_cli")) return true;
  return p.agents.includes("opencode") && !!f.install_path;
};

function matches(p: Item, q: string): boolean {
  if (!q) return true;
  const f = p.facts.kind === "plugin" ? p.facts : null;
  return `${p.name} ${f?.marketplace ?? ""}`.toLowerCase().includes(q.trim().toLowerCase());
}
