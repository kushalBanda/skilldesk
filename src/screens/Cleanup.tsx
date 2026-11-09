import { StatTiles } from "../components/StatTiles";
import { Th, sortBy, useSort } from "../components/SortHeader";
import { PageHeader } from "../components/PageHeader";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { qk, useStorage } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import { useAction } from "../actions/ActionProvider";
import type { AgentStorage } from "../ipc/bindings/AgentStorage";
import type { Candidate } from "../ipc/bindings/Candidate";
import { formatBytes, formatDate, tildePath } from "../lib/format";
import { t } from "../strings";
import { agentColor, OTHER_COLOR } from "../lib/series";

const btn = "h-8 shrink-0 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97] disabled:opacity-50";

/** Agents under this share of the total fold into one "other" segment. */
const MIN_SHARE = 0.004;

const day = (ms: number | null) => (ms ? formatDate(new Date(ms).toISOString()) : "");

export function Cleanup() {
  const q = useStorage();
  const client = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const r = q.data;
  const measure = async () => {
    setError(null);
    setBusy(true);
    try {
      client.setQueryData(qk.storage, await ipc.storageRefresh());
      await client.invalidateQueries({ queryKey: qk.fixes });
    } catch (e) {
      setError(getErrorMessage(e, t.loadFailed));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full overflow-auto page">
      <div className="flex w-full max-w-[1680px] flex-col gap-6">
        <PageHeader
          title={t.cleanupTitle}
          lead={r ? (r.reclaimable_bytes > 0 ? t.cleanupHeader(formatBytes(r.total_bytes), formatBytes(r.reclaimable_bytes)) : t.cleanupHeaderNone(formatBytes(r.total_bytes))) : undefined}
        >
          <button type="button" className={btn} disabled={busy} onClick={() => void measure()}>
            {t.measureAgain}
          </button>
        </PageHeader>
        {q.error || error ? (
          <p role="alert" className="m-0 text-danger">
            {error ?? getErrorMessage(q.error, t.loadFailed)}
          </p>
        ) : null}
        {r ? (
          <>
            <StatTiles
              label={t.cleanupTitle}
              items={[
                { label: t.agentData, value: formatBytes(r.total_bytes), hint: t.acrossAgents(r.agents.length) },
                {
                  label: t.canGo,
                  value: formatBytes(r.reclaimable_bytes),
                  tone: r.reclaimable_bytes > 0 ? "warn" : null,
                  meter: r.total_bytes ? r.reclaimable_bytes / r.total_bytes : 0,
                  hint: t.candidatesN(r.candidates.length),
                },
                { label: t.inTrash, value: r.trash_bytes !== null ? formatBytes(r.trash_bytes) : "—", hint: t.trashHint },
              ]}
            />
            <StorageBar agents={r.agents} total={r.total_bytes} />
            <AgentTable agents={r.agents} total={r.total_bytes} />
            {r.candidates.length ? <Candidates list={r.candidates} /> : null}
            <p className="m-0 text-[12.5px] text-ink-3">
              {r.trash_bytes !== null ? t.trashHolds(formatBytes(r.trash_bytes)) : t.trashNote} {t.measuredAt(formatDate(r.measured_at), r.measure_ms)}
            </p>
          </>
        ) : null}
      </div>
    </div>
  );
}

type Seg = { key: string; name: string; bytes: number; color: string };

function segments(agents: AgentStorage[], total: number): Seg[] {
  const big = agents.filter((a) => a.bytes / Math.max(total, 1) >= MIN_SHARE);
  const small = agents.filter((a) => a.bytes / Math.max(total, 1) < MIN_SHARE);
  const segs: Seg[] = big.map((a) => ({ key: a.agent, name: a.name, bytes: a.bytes, color: agentColor(a.agent) }));
  const rest = small.reduce((n, a) => n + a.bytes, 0);
  if (rest > 0) segs.push({ key: "_other", name: t.otherAgentsN(small.length), bytes: rest, color: OTHER_COLOR });
  return segs;
}

function StorageBar({ agents, total }: { agents: AgentStorage[]; total: number }) {
  const [hover, setHover] = useState<string | null>(null);
  const segs = segments(agents, total);
  return (
    <section aria-label={t.storageByAgent}>
      <div className="flex h-4 w-full gap-[2px]" role="img" aria-label={segs.map((s) => `${s.name} ${formatBytes(s.bytes)}`).join(", ")}>
        {segs.map((s, i) => (
          <div
            key={s.key}
            onMouseEnter={() => setHover(s.key)}
            onMouseLeave={() => setHover(null)}
            title={`${s.name} · ${formatBytes(s.bytes)}`}
            className={["h-full min-w-[3px] transition-opacity", i === 0 ? "rounded-l-[4px]" : "", i === segs.length - 1 ? "rounded-r-[4px]" : "", hover && hover !== s.key ? "opacity-40" : ""].join(" ")}
            style={{ flexGrow: s.bytes, flexBasis: 0, background: s.color }}
          />
        ))}
      </div>
      <ul className="m-0 mt-3 flex list-none flex-wrap gap-x-5 gap-y-1 p-0 text-[13px]">
        {segs.map((s) => (
          <li key={s.key} className="flex items-center gap-1.5" onMouseEnter={() => setHover(s.key)} onMouseLeave={() => setHover(null)}>
            <span className="inline-block h-2.5 w-2.5 rounded-[3px]" style={{ background: s.color }} />
            <span className="font-semibold">{s.name}</span>
            <span className="tnum text-ink-2">{formatBytes(s.bytes)}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function AgentTable({ agents, total }: { agents: AgentStorage[]; total: number }) {
  const { sort, toggle: onSort } = useSort<"agent" | "size" | "what">();
  const rows = sortBy(
    agents.filter((a) => a.bytes / Math.max(total, 1) >= MIN_SHARE),
    sort,
    (a, k) => (k === "agent" ? a.name : k === "size" ? a.bytes : (a.categories[0]?.bytes ?? null)),
  );
  return (
    <table className="w-full table-fixed border-collapse text-left text-[14px]">
      <thead>
        <tr className="text-[12.5px] font-semibold text-ink-3">
          <Th table="cleanup-agents" label={t.colAgent} k="agent" first="asc" sort={sort} onSort={onSort} className="w-44 pr-4" />
          <Th table="cleanup-agents" label={t.colSize} k="size" sort={sort} onSort={onSort} align="right" className="w-24 pr-4" />
          <Th table="cleanup-agents" label={t.colWhat} k="what" sort={sort} onSort={onSort} resize={false} />
        </tr>
      </thead>
      <tbody>
        {rows.map((a) => (
          <tr key={a.agent} className="border-t border-rule align-top">
            <td className="truncate py-2.5 pr-4 font-semibold">{a.name}</td>
            <td className="tnum py-2.5 pr-4 text-right">{formatBytes(a.bytes)}</td>
            <td className="truncate py-2.5 text-ink-2" title={a.roots.map(tildePath).join(", ")}>
              {a.categories
                .slice(0, 4)
                .map((c) => `${t.category[c.category] ?? c.category} ${formatBytes(c.bytes)}`)
                .join(" · ")}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Candidates({ list }: { list: Candidate[] }) {
  const { run } = useAction();
  return (
    <section>
      <h2 className="mb-2 mt-2 text-[20px] font-bold tracking-[-0.02em]">{t.candidatesTitle}</h2>
      <ul className="m-0 flex list-none flex-col p-0">
        {list.map((c) => {
          const [title, note] = t.candidate[c.kind] ?? [c.kind, ""];
          const history = c.kind === "old_transcripts" || c.kind === "orphan_projects";
          const viaPlugins = c.request.type !== "storage_trash";
          return (
            <li key={c.kind} data-row-id={c.kind} className="border-t border-rule py-3">
              <div className="flex items-start gap-4">
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-2">
                    <span className="font-semibold">{title}</span>
                    {history ? <span className="rounded-full bg-sunk px-2 text-[11.5px] font-semibold text-ink-2">{t.historyBadge}</span> : null}
                  </div>
                  <p className="m-0 mt-0.5 text-[13px] text-ink-2">{note}</p>
                  <details className="mt-1">
                    <summary onMouseDown={(e) => e.preventDefault()} className="cursor-default text-[12.5px] font-semibold text-ink-3">{t.showPaths(c.path_count)}</summary>
                    <ul className="selectable scroll-quiet m-0 mt-2 max-h-56 list-none overflow-auto rounded-row bg-sunk px-3 py-2 font-mono text-[11.5px] leading-[20px] text-ink-2">
                      {c.paths.map((p) => (
                        <li key={p} className="truncate" title={p}>
                          {tildePath(p)}
                        </li>
                      ))}
                    </ul>
                    {c.path_count > c.paths.length ? <p className="m-0 mt-1 text-[12px] text-ink-3">{t.morePaths(c.path_count - c.paths.length)}</p> : null}
                  </details>
                </div>
                <div className="w-24 shrink-0 text-right">
                  <div className="tnum font-semibold">{formatBytes(c.bytes)}</div>
                  <div className="tnum text-[12px] text-ink-3" title={t.colNewest}>
                    {day(c.newest)}
                  </div>
                </div>
                <button type="button" className={btn} onClick={() => void run(c.request)} title={viaPlugins ? t.openPlugins : undefined}>
                  {t.moveToTrash}
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
