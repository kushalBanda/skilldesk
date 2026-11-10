import { SearchField } from "../components/SearchField";
import { StatTiles } from "../components/StatTiles";
import { PageHeader } from "../components/PageHeader";
import { AgentStack } from "../components/AgentLogo";
import { Th, sortBy, useSort } from "../components/SortHeader";
import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useItems, useOverview } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import { useAction } from "../actions/ActionProvider";
import type { Item } from "../ipc/bindings/Item";
import type { McpFacts } from "../ipc/bindings/McpFacts";
import type { AgentSummary } from "../ipc/bindings/AgentSummary";
import { FChip } from "../components/FChip";
import { AgentFilter } from "../components/AgentFilter";
import { Switch } from "../components/Switch";
import { DetailSheet, SheetSteps } from "../components/DetailSheet";
import { dotColumns } from "../lib/agents";
import { formatCount, formatDate, tildePath } from "../lib/format";
import { PathLink, PluginName } from "../components/PlaceLink";
import { t } from "../strings";

type StatusFilter = "all" | "unused" | "off" | "keys";
const facts = (i: Item): McpFacts | null => (i.facts.kind === "mcp_server" ? i.facts : null);

export function Mcp() {
  const [params] = useSearchParams();
  const [status, setStatus] = useState<StatusFilter>((params.get("status") as StatusFilter) ?? "all");
  const [agent, setAgent] = useState<string | null>(params.get("agent"));
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(params.get("id"));
  useEffect(() => {
    setStatus((params.get("status") as StatusFilter) ?? "all");
    setAgent(params.get("agent"));
    setSelectedId(params.get("id"));
  }, [params]);
  const { data, error, isPending } = useItems("mcp_server");
  const agents = useOverview().data?.agents ?? [];
  // Row names its agents from the full list (any tier).
  const columns = agents;
  const all = data ?? [];
  const configured = all.filter((s) => !facts(s)?.unconfigured);
  const builtin = all.filter((s) => facts(s)?.unconfigured).sort((a, b) => Number(b.usage?.count ?? 0) - Number(a.usage?.count ?? 0));
  const selected = all.find((s) => s.id === selectedId) ?? null;
  const n = {
    all: configured.length,
    unused: configured.filter((s) => s.status.state === "unused").length,
    off: configured.filter((s) => s.status.state === "off").length,
    keys: configured.filter((s) => facts(s)?.holds_keys).length,
  };
  const { sort, toggle: onSort } = useSort<"name" | "agents" | "calls" | "last" | "on">({ key: "calls", dir: "desc" });
  const ordered = useMemo(
    () =>
      configured
        .filter((s) => (status === "all" ? true : status === "keys" ? facts(s)?.holds_keys : s.status.state === status))
        .filter((s) => !agent || s.agents.includes(agent))
        .filter((s) => !query || s.name.toLowerCase().includes(query.trim().toLowerCase()))
        .sort((a, b) => Number(b.usage?.count ?? 0) - Number(a.usage?.count ?? 0) || a.name.localeCompare(b.name)),
    [configured, status, agent, query],
  );
  const rows = sortBy(ordered, sort, (s, k) =>
    k === "name"
      ? s.name
      : k === "agents"
        ? s.agents.length
        : k === "calls"
          ? Number(s.usage?.count ?? 0)
          : k === "on"
            ? Number(facts(s)?.locations.some((l) => l.enabled) ?? false)
            : (s.usage?.last_used ?? null),
  );
  const totalCalls = all.reduce((x, s) => x + Number(s.usage?.count ?? 0), 0);
  const chip = (k: StatusFilter, label: string) => (
    <FChip label={label} count={n[k]} active={status === k} onClick={() => setStatus(status === k && k !== "all" ? "all" : k)} />
  );

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-6 overflow-auto page">
        <PageHeader
          title={t.mcp}
          search={<SearchField value={query} onChange={setQuery} label={t.searchMcp} count={query ? t.nResults(rows.length) : null} className="w-64" />}
        />
        {error ? (
          <p role="alert" className="text-danger">
            {getErrorMessage(error, t.loadFailed)}
          </p>
        ) : null}
        <StatTiles
          label={t.mcp}
          items={[
            {
              label: t.servers,
              value: formatCount(n.all),
              split: [
                { label: t.called, value: n.all - n.unused - n.off, tone: "ok" },
                { label: t.neverCalled, value: n.unused, tone: "warn" },
                { label: t.off, value: n.off, tone: "muted" },
              ],
            },
            {
              label: t.calls,
              value: formatCount(totalCalls),
              bars: all.map((s) => ({ label: s.name, value: Number(s.usage?.count ?? 0), display: formatCount(Number(s.usage?.count ?? 0)) })),
            },
            {
              label: t.neverCalled,
              value: formatCount(n.unused),
              hint: n.unused ? t.costTokens : t.allCalled,
              tone: n.unused > 0 ? "warn" : null,
              onClick: () => setStatus(status === "unused" ? "all" : "unused"),
              active: status === "unused",
            },
            {
              label: t.withKeys,
              value: formatCount(n.keys),
              hint: t.keysHint,
              onClick: () => setStatus(status === "keys" ? "all" : "keys"),
              active: status === "keys",
            },
          ]}
        />
        <div className="flex flex-wrap items-center gap-2">
          {chip("all", t.all)}
          {chip("unused", t.neverCalled)}
          {chip("off", t.off)}
          {chip("keys", t.withKeys)}
          <AgentFilter agents={agents} items={configured} value={agent} onChange={setAgent} />
        </div>
        {isPending ? null : (
          <table className="w-full table-fixed border-collapse text-left text-[14px]">
            <thead>
              <tr className="text-[12.5px] font-semibold text-ink-3">
                <Th table="mcp" label={t.colName} k="name" first="asc" sort={sort} onSort={onSort} className="pr-4" />
                <Th table="mcp" label={t.agentsCol} k="agents" sort={sort} onSort={onSort} className="w-[150px] pr-2" />
                <Th table="mcp" label={t.calls} k="calls" sort={sort} onSort={onSort} align="right" resize={!selected} className="w-20 pr-4" />
                {selected ? null : <Th table="mcp" label={t.lastCalled} k="last" sort={sort} onSort={onSort} className="w-32 pr-4" />}
                <Th table="mcp" label={t.colOn} k="on" sort={sort} onSort={onSort} align="right" resize={false} className="w-16" />
              </tr>
            </thead>
            <tbody>
              {rows.map((s) => (
                <Row key={s.id} item={s} columns={columns} active={selected?.id === s.id} compact={selected !== null} onOpen={() => setSelectedId(s.id)} />
              ))}
            </tbody>
          </table>
        )}
        {builtin.length ? (
          <div>
            <h2 className="mb-1 mt-4 text-[13px] font-semibold text-ink-2">
              {t.notInConfig} <span className="tnum font-normal text-ink-3">{builtin.length}</span>
            </h2>
            <p className="mb-2 mt-0 text-[13px] text-ink-3">{t.notInConfigNote}</p>
            <ul className="m-0 flex list-none flex-wrap gap-2 p-0">
              {builtin.map((s) => (
                <li key={s.id} className="rounded-full bg-panel px-3 py-1 text-[13px]">
                  {s.name} <span className="tnum text-ink-3">{formatCount(Number(s.usage?.count ?? 0))}</span>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
      </section>
      {selected && !facts(selected)?.unconfigured ? (
        <SheetSteps ids={rows.filter((r) => !facts(r)?.unconfigured).map((r) => r.id)} current={selectedId} go={setSelectedId}>
          <McpSheet key={selected.id} item={selected} agents={agents} onClose={() => setSelectedId(null)} />
        </SheetSteps>
      ) : null}
    </div>
  );
}

function Badges({ item }: { item: Item }) {
  const f = facts(item);
  const st = item.status.state;
  return (
    <>
      {st === "unused" ? <span className="ml-2 rounded-full bg-sunk px-2 py-0.5 text-[11.5px] font-medium text-warn-text">{t.neverCalled}</span> : null}
      {st === "off" ? <span className="ml-2 rounded-full bg-sunk px-2 py-0.5 text-[11.5px] font-medium text-ink-3">{t.off}</span> : null}
      {f?.holds_keys ? (
        <span className="ml-2 rounded-full bg-danger-soft px-2 py-0.5 text-[11.5px] font-medium text-danger" title={t.keysNote}>
          {t.token}
        </span>
      ) : null}
    </>
  );
}

function Row({ item, columns, active, compact, onOpen }: { item: Item; columns: AgentSummary[]; active: boolean; compact: boolean; onOpen: () => void }) {
  const f = facts(item)!;
  return (
    <tr data-row-id={item.id} onClick={onOpen} className={["cursor-default border-t border-rule-2", active ? "bg-accent-soft" : "hover:bg-sunk"].join(" ")}>
      <td className="truncate py-3 pr-4">
        <button type="button" onClick={onOpen} className="font-semibold tracking-[-0.02em]">
          {item.name}
        </button>
        <Badges item={item} />
      </td>
      <td className="py-3 pr-2">
        <AgentStack
          agents={[...new Set(f.locations.map((l) => l.agent))]}
          name={(k) => columns.find((a) => a.key === k)?.display_name ?? k}
          off={(k) => !f.locations.some((l) => l.agent === k && l.enabled)}
        />
      </td>
      <td className="tnum py-3 pr-4 text-right">{formatCount(Number(item.usage?.count ?? 0))}</td>
      {compact ? null : <td className="tnum py-3 pr-4 text-ink-2">{formatDate(item.usage?.last_used)}</td>}
      <td className="py-2 text-right">
        <McpSwitch item={item} name={(k) => columns.find((a) => a.key === k)?.display_name ?? k} />
      </td>
    </tr>
  );
}

/**
 * One switch per server for the places SkillDesk can switch. It shows their
 * state, not the state of places it cannot touch (Claude's own config); those
 * get a dot and a note, so a flip never looks like it did nothing.
 */
function McpSwitch({ item, name }: { item: Item; name: (k: string) => string }) {
  const f = facts(item)!;
  const { run } = useAction();
  const switchable = f.locations.filter((l) => l.switch === "native");
  const fixed = f.locations.filter((l) => l.switch !== "native");
  const on = switchable.some((l) => l.enabled);
  // The new state shows at once; the scan after the write confirms it.
  const [want, setWant] = useState<boolean | null>(null);
  useEffect(() => setWant(null), [on]);
  if (!switchable.length) {
    const any = f.locations.some((l) => l.enabled);
    return (
      <span className="text-[12px] text-ink-3" title={t.mcpNoSwitch}>
        {any ? t.on : t.off}
      </span>
    );
  }
  const shown = want ?? on;
  const stillOn = [...new Set(fixed.filter((l) => l.enabled).map((l) => name(l.agent)))];
  const note = !shown && stillOn.length ? t.mcpStillOn(stillOn.join(", ")) : undefined;
  return (
    <span className="inline-flex items-center gap-2" title={note}>
      {note ? <span aria-label={note} className="h-1.5 w-1.5 rounded-full bg-warn" /> : null}
      <Switch
        on={shown}
        disabled={want !== null}
        label={t.mcpSwitch(item.name, shown)}
        onChange={async (next) => {
          setWant(next);
          try {
            for (const l of switchable.filter((x) => x.enabled !== next)) {
              await run({ type: "mcp_set_enabled", id: item.id, entry: l.entry, on: next });
            }
          } finally {
            // A failed or cancelled write falls back to what is on disk.
            window.setTimeout(() => setWant(null), 1500);
          }
        }}
      />
    </span>
  );
}

function McpSheet({ item, agents, onClose }: { item: Item; agents: AgentSummary[]; onClose: () => void }) {
  const f = facts(item)!;
  const { run } = useAction();
  const name = (k: string) => agents.find((a) => a.key === k)?.display_name ?? k;
  const has = new Set(f.locations.map((l) => l.agent));
  const copyTargets = dotColumns(agents).filter((a) => !has.has(a.key) && a.key !== "claude_code");
  return (
    <DetailSheet title={item.name} onClose={onClose} width={480}>
      <h2 className="m-0 break-words text-[24px] font-bold leading-tight tracking-[-0.03em]">
        {item.name}
        <Badges item={item} />
      </h2>
      <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">
        {t.configuredIn} · {f.locations.length}
      </h3>
      <ul className="m-0 flex list-none flex-col gap-2 p-0">
        {f.locations.map((l) => {
          const switchable = l.switch === "native";
          return (
            <li key={l.entry} className="rounded-row bg-panel px-3 py-2.5">
              <div className="flex items-center gap-2">
                <span className="font-semibold">{name(l.agent)}</span>
                <span className="text-[12.5px] text-ink-3">
                  {l.transport}
                  {l.project ? (
                    <>
                      {" · "}
                      <PathLink path={l.project}>{tildePath(l.project)}</PathLink>
                    </>
                  ) : null}
                  {l.plugin ? (
                    <>
                      {` · ${t.plugin} `}
                      <PluginName id={l.plugin} />
                    </>
                  ) : null}
                </span>
                <span className="ml-auto">
                  {switchable ? (
                    <button
                      type="button"
                      onClick={() =>
                        void run({
                          type: "mcp_set_enabled",
                          id: item.id,
                          entry: l.entry,
                          on: !l.enabled,
                        })
                      }
                      className={[
                        "h-7 rounded-full px-3 text-[12.5px] font-semibold active:scale-[0.97]",
                        l.enabled ? "bg-bg ring-1 ring-rule hover:bg-sunk" : "bg-ink text-bg",
                      ].join(" ")}
                    >
                      {l.enabled ? t.turnOff : t.turnOn}
                    </button>
                  ) : l.switch === "remove" ? (
                    <button
                      type="button"
                      title={t.mcpRemoveNote}
                      onClick={() => void run({ type: "mcp_remove", id: item.id, entry: l.entry }, f.locations.length === 1 ? onClose : undefined)}
                      className="h-7 rounded-full px-3 text-[12.5px] font-semibold text-danger ring-1 ring-rule hover:bg-danger-soft active:scale-[0.97]"
                    >
                      {t.remove}
                    </button>
                  ) : (
                    <span className="text-[12px] text-ink-3">{l.enabled ? t.on : t.off}</span>
                  )}
                </span>
              </div>
              {l.command || l.url ? <div className="selectable mt-1 break-all font-mono text-[12px] text-ink-2">{l.command ?? l.url}</div> : null}
              {l.env_keys.length || l.header_keys.length ? (
                <div className="mt-1 text-[12px] text-ink-3">
                  {t.keysNamed} {[...l.env_keys, ...l.header_keys].join(", ")}
                </div>
              ) : null}
              <div className="selectable mt-1 truncate font-mono text-[11.5px] text-ink-3" title={l.file}>
                {tildePath(l.file)}
              </div>
              {l.switch === "none" ? <div className="mt-1 text-[12px] text-ink-3">{t.claudeJsonNote}</div> : null}
            </li>
          );
        })}
      </ul>
      {copyTargets.length ? (
        <>
          <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">{t.copyTo}</h3>
          <div className="flex flex-wrap gap-2">
            {copyTargets.map((a) => (
              <button
                key={a.key}
                type="button"
                onClick={() =>
                  void run({
                    type: "mcp_copy_to_agent",
                    id: item.id,
                    agent: a.key,
                  })
                }
                className="h-8 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
              >
                {a.display_name}
              </button>
            ))}
          </div>
        </>
      ) : null}
    </DetailSheet>
  );
}
