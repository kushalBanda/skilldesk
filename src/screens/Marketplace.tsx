import { PageHeader } from "../components/PageHeader";
import { SearchField } from "../components/SearchField";
// Marketplace (DESIGN.md §7.21, ADR 0016): skills.sh lists and search, install
// through the library path. The only screen that uses the network; requests
// start when it opens and stop when it closes.
import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { getErrorKind, getErrorMessage } from "../ipc/error";
import { useOverview } from "../ipc/queries";
import { useAction } from "../actions/ActionProvider";
import type { Board } from "../ipc/bindings/Board";
import type { MarketRow } from "../ipc/bindings/MarketRow";
import type { UpdateRow } from "../ipc/bindings/UpdateRow";
import { DetailSheet } from "../components/DetailSheet";
import { FChip } from "../components/FChip";
import { Menu } from "../components/Menu";
import { AgentChips, ClashNotes } from "../components/AgentChips";
import { DEFAULT_ON } from "./InstallSheet";
import { setNetStatus } from "../lib/netStatus";
import { formatBytes, formatCount, formatDate } from "../lib/format";
import { t } from "../strings";

/** Cards per page; each new owner costs one avatar request. */
const PAGE = 60;

type View = Board | "installed" | "updates";
type Pick = {
  source: string;
  skillId: string;
  name: string;
  installs: number | null;
};

const NET = {
  retry: false,
  refetchOnWindowFocus: false,
  staleTime: 5 * 60_000,
} as const;

function when(iso: string | null | undefined): string {
  if (!iso) return "";
  return `${formatDate(iso)}, ${new Date(iso).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`;
}

function sourceLabel(source: string): string {
  return source.includes("/") ? `github.com/${source}` : source;
}

export function Marketplace() {
  const [view, setView] = useState<View>("all_time");
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [pick, setPick] = useState<Pick | null>(null);
  const [from, setFrom] = useState("");
  const [limit, setLimit] = useState(PAGE);
  useEffect(() => {
    const h = setTimeout(() => setQuery(input.trim()), 300);
    return () => clearTimeout(h);
  }, [input]);

  const boardKey: Board = view === "trending" || view === "hot" ? view : "all_time";
  const board = useQuery({
    queryKey: ["market", "board", boardKey],
    queryFn: () => ipc.marketBoard(boardKey),
    ...NET,
  });
  const search = useQuery({
    queryKey: ["market", "search", query],
    queryFn: () => ipc.marketSearch(query),
    enabled: query !== "",
    ...NET,
  });
  const updates = useQuery({
    queryKey: ["market", "updates"],
    queryFn: () => ipc.marketUpdates(false),
    ...NET,
  });

  const offline = board.data?.offline || getErrorKind(board.error) === "network" || getErrorKind(search.error) === "network";
  useEffect(() => {
    if (board.isPending && !board.data) return;
    setNetStatus(offline ? "offline" : "online");
  }, [offline, board.isPending, board.data]);
  useEffect(() => () => setNetStatus(null), []);

  const installed = updates.data?.rows ?? [];
  const nUpdates = installed.filter((r) => r.state.kind === "available").length;
  const list = query ? search : board;
  const all = list.data?.rows ?? [];
  // Sources in this list, most skills first.
  const sources = useMemo(() => {
    const n = new Map<string, number>();
    for (const r of all) n.set(r.skill.source, (n.get(r.skill.source) ?? 0) + 1);
    return [...n].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }, [all]);
  const rows = from ? all.filter((r) => r.skill.source === from) : all;
  // A new list or filter starts at the first page again.
  useEffect(() => setLimit(PAGE), [view, query, from]);
  const max = useMemo(() => Math.max(1, ...all.map((r) => r.skill.installs)), [all]);
  const showBoard = view !== "installed" && view !== "updates";

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-5 overflow-auto page">
        <div>
          <PageHeader
            title={t.marketplace}
            lead={t.marketNote}
            search={
              <SearchField
                value={input}
                onChange={(v) => {
                  setInput(v);
                  if (!showBoard) setView("all_time");
                }}
                label={t.marketSearch}
                busy={!!query && search.isFetching}
                count={query && search.data ? t.nResults(rows.length) : null}
                className="w-72"
              />
            }
          />
        </div>
        {offline && board.data ? (
          <p role="status" className="m-0 max-w-[860px] rounded-row bg-sunk px-3 py-2 text-[13px] text-warn-text">
            {t.offlineBanner(when(board.data.fetched_at))}
          </p>
        ) : board.data?.note ? (
          <p role="status" className="m-0 max-w-[860px] rounded-row bg-sunk px-3 py-2 text-[13px] text-warn-text">
            {t.listNote(board.data.note, when(board.data.fetched_at))}
          </p>
        ) : null}
        <div className="flex flex-wrap items-center gap-2">
          {(
            [
              ["all_time", t.boardAllTime, BOARD_ICONS.all_time],
              ["trending", t.boardTrending, BOARD_ICONS.trending],
              ["hot", t.boardHot, BOARD_ICONS.hot],
            ] as const
          ).map(([k, label, d]) => (
            <FChip
              key={k}
              label={label}
              icon={<Glyph d={d} />}
              active={!query && view === k}
              onClick={() => (setInput(""), setQuery(""), setFrom(""), setView(k))}
            />
          ))}
          {showBoard && sources.length > 1 ? (
            <Menu
              label={t.marketSource}
              value={sources.some(([s]) => s === from) ? from : ""}
              onChange={setFrom}
              icon={<Glyph d={BOARD_ICONS.source} />}
              options={[{ value: "", label: t.allSources, hint: String(all.length) }, ...sources.slice(0, 40).map(([s, n]) => ({ value: s, label: s, hint: String(n) }))]}
            />
          ) : null}
          <span className="mx-1 h-5 w-px bg-rule" />
          <FChip
            label={t.installedHere}
            count={installed.length}
            active={view === "installed"}
            onClick={() => setView(view === "installed" ? "all_time" : "installed")}
          />
          <FChip label={t.updates} count={nUpdates} active={view === "updates"} onClick={() => setView(view === "updates" ? "all_time" : "updates")} />
        </div>

        {showBoard ? (
          <>
            {list.error && !list.data ? (
              <p role="alert" className="m-0 text-danger">
                {getErrorMessage(list.error, t.loadFailed)}
              </p>
            ) : null}
            {query && search.data && rows.length === 0 ? <p className="m-0 text-ink-2">{t.noResults}</p> : null}
            <ol
              className="m-0 grid list-none gap-3 p-0"
              style={{
                gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))",
              }}
            >
              {rows.slice(0, limit).map((r) => (
                <Row
                  key={r.skill.id}
                  row={r}
                  max={max}
                  active={pick?.source === r.skill.source && pick.skillId === r.skill.skill_id}
                  onSource={() => setFrom(from === r.skill.source ? "" : r.skill.source)}
                  sourceOn={from === r.skill.source}
                  onOpen={() =>
                    setPick({
                      source: r.skill.source,
                      skillId: r.skill.skill_id,
                      name: r.skill.name,
                      installs: r.skill.installs,
                    })
                  }
                />
              ))}
            </ol>
            {rows.length > limit ? (
              <button
                type="button"
                onClick={() => setLimit(limit + PAGE)}
                className="h-9 self-center rounded-full bg-bg px-5 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
              >
                {t.showMoreOf(Math.min(PAGE, rows.length - limit), rows.length - limit)}
              </button>
            ) : null}
            {rows.length ? <p className="m-0 text-[12.5px] text-ink-3">{t.marketFooter}</p> : null}
          </>
        ) : (
          <InstalledList
            rows={view === "updates" ? installed.filter((r) => r.state.kind === "available") : installed}
            checkedAt={updates.data?.checked_at ?? null}
            note={updates.data?.note ?? (updates.error ? getErrorMessage(updates.error, t.loadFailed) : null)}
            onPick={(r) =>
              setPick({
                source: r.skill.source,
                skillId: r.skill.skill_id,
                name: r.skill.name,
                installs: null,
              })
            }
          />
        )}
      </section>
      {pick ? <MarketSheet key={`${pick.source}/${pick.skillId}`} pick={pick} offline={!!offline} onClose={() => setPick(null)} /> : null}
    </div>
  );
}

const BOARD_ICONS = {
  all_time: "M8 2.75A5.25 5.25 0 108 13.25 5.25 5.25 0 008 2.75zM8 5v3l2 1.5",
  trending: "M2.5 11.5l3.75-3.75 2.5 2.5L13.5 5M10 5h3.5v3.5",
  hot: "M8 14c2.5 0 4.25-1.75 4.25-4.25C12.25 6.5 8.75 5.25 8.5 2 6.5 3.5 5.75 5.75 6.25 7.5 5.25 7 4.75 6.25 4.75 5.5 4 6.5 3.75 8 3.75 9.75 3.75 12.25 5.5 14 8 14z",
  source: "M3 13.25V3.5a1 1 0 011-1h8a1 1 0 011 1v8H4.5A1.5 1.5 0 003 13a1.5 1.5 0 001.5 1.5H13M5.5 5.5h5",
} as const;

function Glyph({ d }: { d: string }) {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="text-ink-3">
      <path d={d} />
    </svg>
  );
}

const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });

/**
 * The publisher's mark: the GitHub owner's avatar (fetched by the Rust core,
 * cached a week, shown as a data URL), else a letter tile. Site-published
 * skills get the letter tile.
 */
export function SourceAvatar({ source, size = 36 }: { source: string; size?: number }) {
  const owner = source.includes("/") ? source.split("/")[0]! : "";
  const q = useQuery({
    queryKey: ["market", "avatar", owner.toLowerCase()],
    queryFn: () => ipc.marketAvatar(owner),
    enabled: owner !== "",
    retry: false,
    staleTime: Infinity,
    gcTime: Infinity,
    refetchOnWindowFocus: false,
  });
  const letter = (owner || source).replace(/^www\./, "").charAt(0).toUpperCase();
  return (
    <span
      aria-hidden="true"
      className="grid shrink-0 place-items-center overflow-hidden rounded-[10px] bg-sunk text-ink-2 ring-1 ring-rule"
      style={{ width: size, height: size, fontSize: Math.round(size * 0.42) }}
    >
      {q.data ? <img src={q.data} alt="" width={size} height={size} className="h-full w-full object-cover" draggable={false} /> : <b className="font-semibold">{letter}</b>}
    </span>
  );
}

function Row({
  row,
  max,
  active,
  sourceOn,
  onSource,
  onOpen,
}: {
  row: MarketRow;
  max: number;
  active: boolean;
  sourceOn: boolean;
  onSource: () => void;
  onOpen: () => void;
}) {
  const s = row.skill;
  const st = row.state;
  const share = Math.max(0.02, s.installs / max);
  const done = st.kind === "installed" && !st.update;
  return (
    <li className="min-w-0">
      <div
        className={[
          "group relative flex h-full flex-col gap-3.5 rounded-card p-4 ring-1 transition-[background-color,transform,box-shadow] duration-150 active:scale-[0.985]",
          active ? "bg-accent-soft ring-accent" : "bg-panel ring-rule-2 hover:bg-sunk hover:ring-rule",
        ].join(" ")}
      >
        {/* The whole card opens the sheet; the source chip and the action button sit above this layer. */}
        <button type="button" onClick={onOpen} aria-label={`${s.name}, ${s.source}, ${t.rankN(row.rank)}`} className="absolute inset-0 rounded-card" />
        <div className="pointer-events-none flex items-start gap-3">
          <SourceAvatar source={s.source} />
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-2">
              <span className="truncate text-[15px] font-semibold tracking-[-0.015em] text-ink">{s.name}</span>
              {st.kind === "installed" ? (
                <span className="inline-flex shrink-0 items-center gap-1 whitespace-nowrap rounded-full bg-[var(--ok-soft)] px-2 py-0.5 text-[11.5px] font-medium text-ok">
                  {st.update ? null : (
                    <svg width="10" height="10" viewBox="0 0 12 12" aria-hidden="true">
                      <path d="M2.5 6.5l2.25 2.25L9.5 3.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
                    </svg>
                  )}
                  {st.update ? t.updateAvailable : t.installed}
                </span>
              ) : null}
            </span>
            <button
              type="button"
              onClick={onSource}
              aria-pressed={sourceOn}
              title={sourceOn ? t.allSources : t.onlyFrom(s.source)}
              className={[
                "pointer-events-auto relative -ml-1 mt-0.5 block max-w-full truncate rounded-[6px] px-1 text-left font-mono text-[12px] transition-colors",
                sourceOn ? "bg-accent-soft text-accent-text" : "text-ink-3 hover:bg-rule-2 hover:text-ink-2",
              ].join(" ")}
            >
              {s.source}
            </button>
          </span>
          <span className="tnum shrink-0 pt-0.5 text-[12px] font-medium text-ink-3">#{row.rank}</span>
        </div>
        <div className="pointer-events-none mt-auto flex items-center gap-3">
          <span className="min-w-0 flex-1" title={t.installsCount(formatCount(s.installs))}>
            <span className="tnum flex items-center gap-1.5 text-[13px]">
              <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="text-ink-3">
                <path d="M8 2.5v8M4.75 7.5L8 10.75l3.25-3.25M3 13.5h10" />
              </svg>
              <b className="font-semibold text-ink">{compact.format(s.installs)}</b>
              <span className="text-ink-3">{t.installsWord}</span>
            </span>
            <span className="mt-1.5 block h-1 overflow-hidden rounded-full bg-rule-2">
              <span className="block h-full rounded-full bg-accent" style={{ width: `${share * 100}%` }} />
            </span>
          </span>
          {done ? null : (
            <button
              type="button"
              onClick={onOpen}
              className="pointer-events-auto relative h-8 shrink-0 rounded-full bg-bg px-4 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
            >
              {st.kind === "installed" ? t.update : t.install}
            </button>
          )}
        </div>
      </div>
    </li>
  );
}

function InstalledList({
  rows,
  checkedAt,
  note,
  onPick,
}: {
  rows: UpdateRow[];
  checkedAt: string | null;
  note: string | null;
  onPick: (r: UpdateRow) => void;
}) {
  const qc = useQueryClient();
  const [checking, setChecking] = useState(false);
  const check = async () => {
    setChecking(true);
    try {
      qc.setQueryData(["market", "updates"], await ipc.marketUpdates(true));
    } catch (e) {
      qc.setQueryData(["market", "updates"], (old: unknown) => old);
      console.warn(getErrorMessage(e, t.loadFailed));
    } finally {
      setChecking(false);
    }
  };
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3 text-[13px] text-ink-3">
        <button
          type="button"
          disabled={checking}
          onClick={() => void check()}
          className="h-8 rounded-full bg-bg px-4 font-semibold text-ink ring-1 ring-rule hover:bg-sunk active:scale-[0.97] disabled:opacity-50"
        >
          {checking ? t.checking : t.checkUpdates}
        </button>
        {checkedAt ? <span>{t.checkedAt(when(checkedAt))}</span> : null}
      </div>
      {note ? <p className="m-0 rounded-row bg-sunk px-3 py-2 text-[13px] text-warn-text">{note}</p> : null}
      {rows.length === 0 ? <p className="m-0 text-ink-2">{t.noneInstalledHere}</p> : null}
      <ul className="m-0 flex list-none flex-col p-0">
        {rows.map((r) => (
          <li key={r.skill.name} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-4 rounded-row px-3 py-2 hover:bg-sunk">
            <button type="button" onClick={() => onPick(r)} className="min-w-0 text-left">
              <span className="block truncate font-semibold">{r.skill.name}</span>
              <span className="block truncate font-mono text-[12px] text-ink-3">
                {r.skill.source}
                {r.skill.revision ? ` · ${t.revisionAt(r.skill.revision)}` : ""}
              </span>
            </button>
            <span className="flex items-center gap-2">
              <span
                className={[
                  "rounded-full px-2 py-0.5 text-[11.5px] font-medium",
                  r.state.kind === "available" ? "bg-accent-soft text-accent-text" : "bg-sunk text-ink-2",
                ].join(" ")}
              >
                {r.state.kind === "available" ? t.updateAvailable : r.state.kind === "current" ? t.upToDate : t.notChecked}
              </span>
              {r.state.kind === "available" ? (
                <button
                  type="button"
                  onClick={() => onPick(r)}
                  className="h-8 rounded-full bg-bg px-4 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
                >
                  {t.update}
                </button>
              ) : null}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function MarketSheet({ pick, offline, onClose }: { pick: Pick; offline: boolean; onClose: () => void }) {
  const agents = useOverview().data?.agents ?? [];
  const { run } = useAction();
  const qc = useQueryClient();
  const fetched = useQuery({
    queryKey: ["market-fetch", pick.source, pick.skillId],
    queryFn: () => ipc.marketFetch(pick.source, pick.skillId),
    retry: false,
    gcTime: 0,
    refetchOnWindowFocus: false,
  });
  const f = fetched.data;
  const p = f?.preview;
  const [chosen, setChosen] = useState<string[] | null>(null);
  const [showMd, setShowMd] = useState(false);
  const linked = p ? p.targets.filter((x) => x.existing.kind === "linked").map((x) => x.key) : [];
  const picked = chosen ?? (p ? (p.in_library && linked.length ? linked : p.targets.filter((x) => DEFAULT_ON.includes(x.key)).map((x) => x.key)) : []);
  const netDown = offline || getErrorKind(fetched.error) === "network";
  const host = pick.source.includes("/") ? "GitHub" : pick.source;

  return (
    <DetailSheet title={pick.name} onClose={onClose} width={440}>
      <div className="flex items-center gap-3">
        <SourceAvatar source={pick.source} size={44} />
        <div className="min-w-0">
          <h2 className="m-0 break-words text-[24px] font-bold leading-tight tracking-[-0.03em]">{pick.name}</h2>
          <p className="selectable mb-0 mt-0.5 truncate font-mono text-[12.5px] text-ink-3">{sourceLabel(pick.source)}</p>
        </div>
      </div>
      <button
        type="button"
        onClick={() => void ipc.marketOpenPage(pick.source, pick.skillId)}
        className="mt-2 inline-flex items-center gap-1 text-[13px] font-semibold text-accent-text hover:underline"
      >
        {t.viewOnSkillsSh}
        <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M4.5 2.5h5v5M9.5 2.5l-7 7" />
        </svg>
      </button>
      {pick.installs !== null ? <p className="mb-0 mt-1 text-[13px] text-ink-2">{t.installsCount(formatCount(pick.installs))}</p> : null}
      {!pick.source.includes("/") ? <p className="mb-0 mt-1 text-[13px] text-ink-2">{t.fromSite}</p> : null}

      {fetched.isPending ? <p className="mt-5 text-ink-2">{t.downloading(host)}</p> : null}
      {fetched.error ? (
        <div className="mt-5 flex flex-col items-start gap-2">
          <p role="alert" className="m-0 text-[13px] text-danger">
            {getErrorMessage(fetched.error, t.loadFailed)}
          </p>
          <button type="button" onClick={() => void fetched.refetch()} className="text-[13px] font-semibold text-accent-text hover:underline">
            {t.retry}
          </button>
        </div>
      ) : null}
      {p && f ? (
        <>
          {p.description ? <p className="mt-4 text-ink-2">{p.description}</p> : null}
          <p className="mt-2 text-[12.5px] text-ink-3">
            {formatCount(p.files)} {p.files === 1 ? t.file : t.files} · {formatBytes(p.bytes)}
            {f.path ? ` · ${f.path}` : ""}
            {f.revision ? ` · ${t.revisionAt(f.revision)}` : ""}
          </p>
          <button type="button" onClick={() => setShowMd(!showMd)} className="text-[13px] font-semibold text-accent-text hover:underline">
            {showMd ? t.hideSkillMd : t.previewSkillMd}
          </button>
          {showMd ? (
            <pre className="selectable mt-2 max-h-[320px] overflow-auto whitespace-pre-wrap break-words rounded-row bg-sunk p-3 font-mono text-[11.5px] leading-[1.5]">
              {f.skill_md}
            </pre>
          ) : null}
          <ClashNotes name={p.name} inLibrary={p.in_library} targets={p.targets} picked={picked} agents={agents} />
          <AgentChips targets={p.targets} picked={picked} onChange={setChosen} agents={agents} />
          <h3 className="mb-1 mt-6 text-[12.5px] font-semibold text-ink-3">{t.whatInstallDoes}</h3>
          <ol className="m-0 flex flex-col gap-1 pl-5 text-[13px] text-ink-2">
            {t.installSteps(p.name).map((s) => (
              <li key={s}>{s}</li>
            ))}
          </ol>
        </>
      ) : null}
      {!p && fetched.error && !netDown ? null : (
        <button
          type="button"
          disabled={!p}
          onClick={() =>
            p &&
            void run({ type: "library_install", from: p.source, agents: picked }, () => {
              void qc.invalidateQueries({ queryKey: ["market"] });
              onClose();
            })
          }
          className="mt-6 h-11 w-full rounded-full bg-ink font-semibold text-bg active:scale-[0.98] disabled:opacity-40"
        >
          {netDown && !p ? t.waitingConnection : p?.in_library ? t.updateAndKeep : picked.length ? t.installAndTurnOn(picked.length) : t.installLibraryOnly}
        </button>
      )}
    </DetailSheet>
  );
}
