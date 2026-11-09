import { StatTiles } from "../components/StatTiles";
import { Th, sortBy, useSort, type Sort } from "../components/SortHeader";
import { PageHeader } from "../components/PageHeader";
import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { qk, useSecrets } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import { useAction } from "../actions/ActionProvider";
import type { Secret } from "../ipc/bindings/Secret";
import { FChip } from "../components/FChip";
import { DetailSheet, SheetSteps } from "../components/DetailSheet";
import { HoldButton } from "../components/HoldButton";
import { formatCount, formatDate, tildePath } from "../lib/format";
import { t } from "../strings";

const btn = "h-8 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97] disabled:opacity-50";
const REVEAL_MS = 10_000;

const day = (ms: number | null) => (ms ? formatDate(new Date(ms).toISOString()) : "");
const kindName = (k: string) => t.kind[k] ?? k;

/** Place names of a value, most common first ("Claude transcript, Cursor chat"). */
function placeSummary(s: Secret): string {
  const n = new Map<string, number>();
  for (const p of s.places) n.set(p.place, (n.get(p.place) ?? 0) + 1);
  return [...n.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([p]) => t.placeName[p] ?? p)
    .join(", ");
}

export function Secrets() {
  const q = useSecrets();
  const all = q.data ?? [];
  const [kind, setKind] = useState("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const likely = all.filter((s) => s.confidence === "likely" && !s.ignored);
  const possible = all.filter((s) => s.confidence === "possible" && !s.ignored);
  const ignored = all.filter((s) => s.ignored);
  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const s of likely) c[s.kind] = (c[s.kind] ?? 0) + 1;
    return c;
  }, [likely]);
  const places = new Set(likely.flatMap((s) => s.places.map((p) => `${p.path}\u0000${p.chat ?? ""}`))).size;
  const { sort, toggle: onSort } = useSort<SecretKey>();
  const order = (list: Secret[]) =>
    sortBy(list, sort, (s, k) => (k === "secret" ? s.masked : k === "type" ? kindName(s.kind) : k === "places" ? s.place_count : s.last_seen));
  const rows = order(kind === "all" ? likely : likely.filter((s) => s.kind === kind));
  const selected = selectedId ? (all.find((s) => s.id === selectedId) ?? null) : null;
  const compact = selected !== null;
  const table = (list: Secret[]) => (
    <SecretTable list={order(list)} compact={compact} selected={selected?.id ?? null} onPick={setSelectedId} sort={sort} onSort={onSort} />
  );

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-6 overflow-auto page">
        <PageHeader title={t.secretsTitle} lead={q.isSuccess ? (likely.length ? t.secretsHeader(likely.length, places) : t.secretsNone) : undefined} />
        {q.error ? (
          <p role="alert" className="text-danger">
            {getErrorMessage(q.error, t.loadFailed)}
          </p>
        ) : null}
        {q.isSuccess && likely.length ? (
          <StatTiles
            label={t.secretsTitle}
            items={[
              {
                label: t.secretsLikely,
                value: formatCount(likely.length),
                tone: "danger",
                hint: t.secretsLikelyHint,
              },
              {
                label: t.secretsPlaces,
                value: formatCount(places),
                // Where they sit: transcripts, file history, config files…
                bars: [...new Set(likely.flatMap((x) => x.places.map((p) => p.place)))].map((pl) => ({
                  label: pl,
                  value: new Set(likely.flatMap((x) => x.places.filter((p) => p.place === pl).map((p) => `${p.path}\u0000${p.chat ?? ""}`))).size,
                })),
              },
              { label: t.secretsPossible, value: formatCount(possible.length), hint: t.secretsPossibleHint },
              { label: t.secretsIgnored, value: formatCount(ignored.length), hint: t.secretsIgnoredHint },
            ]}
          />
        ) : null}
        {likely.length ? (
          <div className="flex flex-wrap items-center gap-2">
            <FChip label={t.all} count={likely.length} active={kind === "all"} onClick={() => setKind("all")} />
            {Object.entries(counts)
              .sort((a, b) => b[1] - a[1])
              .map(([k, n]) => (
                <FChip key={k} label={kindName(k)} count={n} active={kind === k} onClick={() => setKind(kind === k ? "all" : k)} />
              ))}
          </div>
        ) : null}
        {rows.length ? table(rows) : null}
        {possible.length ? (
          <details className="group">
            <summary className="cursor-default text-[14px] font-semibold text-ink-2">{t.possibleSection(possible.length)}</summary>
            <p className="mb-2 mt-2 text-[12.5px] text-ink-3">{t.possibleNote}</p>
            {table(possible)}
          </details>
        ) : null}
        {ignored.length ? (
          <details>
            <summary className="cursor-default text-[14px] font-semibold text-ink-2">{t.ignoredSection(ignored.length)}</summary>
            <div className="mt-2">{table(ignored)}</div>
          </details>
        ) : null}
        <p className="m-0 text-[12.5px] text-ink-3">{t.secretsNote}</p>
      </section>
      {selected ? (
        <SheetSteps ids={rows.map((r) => r.id)} current={selectedId} go={setSelectedId}>
          <SecretSheet key={selected.id} secret={selected} onClose={() => setSelectedId(null)} />
        </SheetSteps>
      ) : null}
    </div>
  );
}

type SecretKey = "secret" | "type" | "places" | "last";

function SecretTable({
  list,
  compact,
  selected,
  onPick,
  sort,
  onSort,
}: {
  list: Secret[];
  compact: boolean;
  selected: string | null;
  onPick: (id: string) => void;
  sort: Sort<SecretKey>;
  onSort: (k: SecretKey, first: "asc" | "desc") => void;
}) {
  return (
    <table className="w-full table-fixed border-collapse text-left text-[14px]">
      <thead>
        <tr className="text-[12.5px] font-semibold text-ink-3">
          <Th table="secrets" label={t.colSecret} k="secret" first="asc" sort={sort} onSort={onSort} className="pr-4" />
          {compact ? null : <Th table="secrets" label={t.colType} k="type" first="asc" sort={sort} onSort={onSort} className="w-32 pr-4" />}
          <Th table="secrets" label={t.colPlaces} k="places" sort={sort} onSort={onSort} resize={!compact} className="w-[38%] pr-4" />
          {compact ? null : <Th table="secrets" label={t.colLastSeen} k="last" sort={sort} onSort={onSort} resize={false} className="w-28" />}
        </tr>
      </thead>
      <tbody>
        {list.map((s) => (
          <tr
            key={s.id}
            data-row-id={s.id}
            onClick={() => onPick(s.id)}
            aria-selected={selected === s.id}
            className={["cursor-default border-t border-rule align-top", selected === s.id ? "bg-sunk" : "hover:bg-panel"].join(" ")}
          >
            <td className="truncate py-2.5 pr-4 font-mono text-[13px]">{s.masked}</td>
            {compact ? null : <td className="truncate py-2.5 pr-4 text-ink-2">{kindName(s.kind)}</td>}
            <td className="truncate py-2.5 pr-4 text-ink-2" title={placeSummary(s)}>
              <span className="tnum font-semibold text-ink">{formatCount(s.place_count)}</span>
              <span className="ml-2 text-[12.5px] text-ink-3">{placeSummary(s)}</span>
            </td>
            {compact ? null : <td className="tnum py-2.5 text-ink-3">{day(s.last_seen)}</td>}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function SecretSheet({ secret: s, onClose }: { secret: Secret; onClose: () => void }) {
  const { run } = useAction();
  const client = useQueryClient();
  const [value, setValue] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setValue(null);
    setError(null);
  }, [s.id]);
  useEffect(() => {
    if (value === null) return;
    const id = window.setTimeout(() => setValue(null), REVEAL_MS);
    return () => window.clearTimeout(id);
  }, [value]);
  const reveal = async () => {
    setError(null);
    setBusy(true);
    try {
      setValue(await ipc.secretReveal(s.id));
    } catch (e) {
      setError(getErrorMessage(e, t.loadFailed));
    } finally {
      setBusy(false);
    }
  };
  const ignore = async () => {
    setError(null);
    try {
      await ipc.secretIgnore(s.id, !s.ignored);
      await client.invalidateQueries({ queryKey: qk.secrets });
      await client.invalidateQueries({ queryKey: qk.fixes });
    } catch (e) {
      setError(getErrorMessage(e, t.loadFailed));
    }
  };
  const files = s.places.filter((p) => p.store === "text_file");
  const dbs = s.places.filter((p) => p.store === "agent_db");
  return (
    <DetailSheet title={s.masked} onClose={onClose} width={500}>
      <h2 className="m-0 break-all font-mono text-[20px] font-bold leading-tight">{value ?? s.masked}</h2>
      <p className="mt-2 text-[13px] text-ink-3">
        <span className="font-semibold text-ink-2">{kindName(s.kind)}</span>
        {s.confidence === "possible" ? <span> · {t.possibleSection(1).replace(/^1 /, "")}</span> : null}
      </p>
      <dl className="mt-4 grid grid-cols-3 gap-4">
        <Fact label={t.firstSeen} value={day(s.first_seen)} />
        <Fact label={t.colLastSeen} value={day(s.last_seen)} />
        <Fact label={t.occurrences} value={formatCount(s.occurrences)} />
      </dl>
      <div className="mt-5 flex flex-wrap items-center gap-2">
        {value === null ? (
          <button type="button" className={btn} disabled={busy || files.length === 0} onClick={() => void reveal()} title={t.revealNote}>
            {t.reveal}
          </button>
        ) : (
          <button type="button" className={btn} onClick={() => setValue(null)}>
            {t.hideNow}
          </button>
        )}
        <button type="button" className={btn} onClick={() => void ignore()}>
          {s.ignored ? t.unignore : t.ignore}
        </button>
      </div>
      <p className="mb-0 mt-2 text-[12.5px] text-ink-3">{t.revealNote}</p>
      {error ? (
        <p role="alert" className="mb-0 mt-2 text-[12.5px] text-danger">
          {error}
        </p>
      ) : null}
      <div className="mt-5 rounded-row bg-sunk p-3">
        <h3 className="m-0 text-[13px] font-semibold">{t.rotate}</h3>
        <p className="mb-0 mt-1 text-[12.5px] text-ink-2">
          {t.rotateNote} <span className="selectable font-semibold text-ink">{s.rotate_at}</span>
        </p>
      </div>
      {files.length ? (
        <div className="mt-4">
          <HoldButton label={t.redactHold} onDone={() => void run({ type: "secret_redact", id: s.id })} />
          <p className="mb-0 mt-2 text-[12.5px] text-ink-3">{t.redactNote}</p>
        </div>
      ) : null}
      <h3 className="mb-2 mt-6 text-[12.5px] font-semibold text-ink-3">
        {t.colPlaces} · {formatCount(s.place_count)}
      </h3>
      <ul className="m-0 flex list-none flex-col gap-2 p-0">
        {[...files, ...dbs].map((p, i) => (
          <li key={`${p.path}-${p.chat ?? ""}-${i}`} className="text-[13px]">
            <div className="flex items-baseline gap-2">
              <span className="font-semibold">{t.placeName[p.place] ?? p.place}</span>
              {p.count > 1 ? <span className="tnum text-ink-3">×{p.count}</span> : null}
              {p.store === "agent_db" ? <span className="text-[12px] text-ink-3">· {t.dbPlace}</span> : null}
              <button
                type="button"
                onClick={() => void ipc.revealInFinder(p.path)}
                className="ml-auto shrink-0 text-[12px] font-semibold text-ink-2 hover:text-ink"
              >
                {t.showInFinder}
              </button>
            </div>
            {p.chat ? <div className="truncate text-ink-2">{p.chat}</div> : null}
            <div className="selectable truncate font-mono text-[11.5px] text-ink-3" title={p.path}>
              {tildePath(p.path)}
            </div>
          </li>
        ))}
      </ul>
      {s.place_count > s.places.length ? <p className="text-[12.5px] text-ink-3">{t.morePlaces(s.place_count - s.places.length)}</p> : null}
    </DetailSheet>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-[12px] font-semibold text-ink-3">{label}</dt>
      <dd className="tnum m-0 mt-0.5 text-[14px] font-semibold">{value || "—"}</dd>
    </div>
  );
}
