import { StatTiles } from "../components/StatTiles";
import { PageHeader } from "../components/PageHeader";
import { useSearchParams } from "react-router-dom";
import { PathLink } from "../components/PlaceLink";
import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { useSessions } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import type { Session } from "../ipc/bindings/Session";
import { FChip } from "../components/FChip";
import { DetailSheet, SheetSteps } from "../components/DetailSheet";
import { AgentLogo } from "../components/AgentLogo";
import { Th, sortBy, useSort } from "../components/SortHeader";
import { SearchField } from "../components/SearchField";
import { formatCount, formatDate, tildePath } from "../lib/format";
import { t } from "../strings";

const PAGE = 200;
const btn = "h-8 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97] disabled:opacity-50";
const primary = "h-8 rounded-full bg-ink px-4 text-[13px] font-semibold text-bg active:scale-[0.97] disabled:opacity-50";

const day = (ms: number | null) => (ms ? formatDate(new Date(ms).toISOString()) : "");
const folder = (p: string | null) => (p ? (p.split("/").filter(Boolean).pop() ?? p) : "");

/** Search terms arrive between U+E000 and U+E001. */
function Snippet({ text }: { text: string }) {
  const parts = text.replace(/\s+/g, " ").split(/([^]*)/);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("") ? (
          <mark key={i} className="rounded-[3px] bg-accent/20 px-0.5 text-ink">
            {p.slice(1, -1)}
          </mark>
        ) : (
          <span key={i}>{p}</span>
        ),
      )}
    </>
  );
}

export function Sessions() {
  const q = useSessions();
  const all = q.data ?? [];
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");

  const [shown, setShown] = useState(PAGE);
  const [params] = useSearchParams();
  const [agent, setAgent] = useState<string>(params.get("agent") ?? "all");
  const [selectedId, setSelectedId] = useState<string | null>(params.get("id"));
  useEffect(() => {
    setSelectedId(params.get("id"));
    setAgent(params.get("agent") ?? "all");
  }, [params]);
  useEffect(() => {
    const id = setTimeout(() => setQuery(input.trim()), 180);
    return () => clearTimeout(id);
  }, [input]);
  useEffect(() => setShown(PAGE), [query, agent]);
  const search = useQuery({
    queryKey: ["sessionSearch", query],
    queryFn: () => ipc.sessionsSearch(query),
    enabled: query.length > 0,
  });

  const byId = useMemo(() => new Map(all.map((s) => [s.id, s])), [all]);
  const snippets = useMemo(() => new Map((search.data ?? []).map((h) => [h.id, h.snippet])), [search.data]);
  const base: Session[] = query ? (search.data ?? []).map((h) => byId.get(h.id)).filter((s): s is Session => !!s) : all;
  const { sort, toggle: onSort } = useSort<"title" | "project" | "prompts" | "last">();
  const rows = sortBy(agent === "all" ? base : base.filter((s) => s.origin === agent), sort, (s, k) =>
    k === "title" ? s.title : k === "project" ? folder(s.project) || null : k === "prompts" ? s.prompts : (s.ended ?? s.started),
  );
  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const s of base) c[s.origin] = (c[s.origin] ?? 0) + 1;
    return c;
  }, [base]);
  const prompts = all.reduce((n, s) => n + s.prompts, 0);
  const projects = new Set(all.map((s) => s.project).filter(Boolean)).size;
  const selected = selectedId ? (byId.get(selectedId) ?? null) : null;
  const originAgent = useMemo(() => {
    const m = new Map<string, string>();
    for (const s of all) if (!m.has(s.origin)) m.set(s.origin, s.agent);
    return m;
  }, [all]);
  const compact = selected !== null;

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col gap-6 overflow-auto page">
        <PageHeader
          title={t.sessionsTitle}
          search={
            <SearchField
              value={input}
              onChange={setInput}
              label={t.sessionSearch}
              busy={!!query && search.isFetching}
              count={query && search.isSuccess ? t.nResults(rows.length) : null}
              className="w-80"
            />
          }
        />
        {q.error ? (
          <p role="alert" className="text-danger">
            {getErrorMessage(q.error, t.loadFailed)}
          </p>
        ) : null}
        <StatTiles
          label={t.sessionsTitle}
          items={[
            {
              label: t.sessionsLabel,
              value: formatCount(all.length),
              bars: [...originAgent.entries()].map(([o, a]) => ({
                label: t.origin[o] ?? o,
                value: all.filter((s) => s.origin === o).length,
                icon: <AgentLogo agent={a} size={13} />,
              })),
            },
            {
              label: t.prompts,
              value: formatCount(prompts),
              hint: t.perSession(all.length ? Math.round((prompts / all.length) * 10) / 10 : 0),
            },
            {
              label: t.projects,
              value: formatCount(projects),
              bars: [...new Set(all.map((s) => s.project).filter((p): p is string => !!p))].map((p) => ({
                label: folder(p),
                value: all.filter((s) => s.project === p).length,
              })),
            },
          ]}
        />
        <div className="flex flex-wrap items-center gap-2">
          <FChip label={t.all} count={base.length} active={agent === "all"} onClick={() => setAgent("all")} />
          {Object.keys(t.origin)
            .filter((o) => counts[o])
            .map((o) => (
              <FChip
                key={o}
                icon={originAgent.get(o) ? <AgentLogo agent={originAgent.get(o)!} size={14} /> : undefined}
                label={t.origin[o] ?? o}
                count={counts[o]}
                active={agent === o}
                onClick={() => setAgent(agent === o ? "all" : o)}
              />
            ))}
        </div>
        {query && search.isSuccess && rows.length === 0 ? <p className="m-0 text-ink-3">{t.noMatches}</p> : null}
        {q.isPending ? null : rows.length ? (
          <table className="w-full table-fixed border-collapse text-left text-[14px]">
            <thead>
              <tr className="text-[12.5px] font-semibold text-ink-3">
                <Th table="sessions" label={t.colSession} k="title" first="asc" sort={sort} onSort={onSort} className="pr-4" />
                {compact ? null : <Th table="sessions" label={t.colProject} k="project" first="asc" sort={sort} onSort={onSort} className="w-40 pr-4" />}
                <Th table="sessions" label={t.prompts} k="prompts" sort={sort} onSort={onSort} align="right" resize={!compact} className="w-20 pr-4" />
                {compact ? null : <Th table="sessions" label={t.colLast} k="last" sort={sort} onSort={onSort} resize={false} className="w-28" />}
              </tr>
            </thead>
            <tbody>
              {rows.slice(0, shown).map((s) => (
                <tr
                  key={s.id}
                  data-row-id={s.id}
                  onClick={() => setSelectedId(s.id)}
                  aria-selected={selected?.id === s.id}
                  className={["cursor-default border-t border-rule align-top", selected?.id === s.id ? "bg-sunk" : "hover:bg-panel"].join(" ")}
                >
                  <td className="py-2.5 pr-4">
                    <div className="flex min-w-0 items-center gap-2.5">
                      <AgentLogo agent={s.agent} size={18} label={t.origin[s.origin] ?? s.origin} />
                      <span className="truncate font-medium">{s.title}</span>
                    </div>
                    {snippets.get(s.id) ? (
                      <div className="mt-0.5 line-clamp-2 pl-[28px] text-[12.5px] text-ink-3">
                        <Snippet text={snippets.get(s.id)!} />
                      </div>
                    ) : null}
                  </td>
                  {compact ? null : (
                    <td className="truncate py-2.5 pr-4 text-ink-2" title={s.project ?? ""}>
                      {folder(s.project)}
                    </td>
                  )}
                  <td className="tnum py-2.5 pr-4 text-right text-ink-2">{formatCount(s.prompts)}</td>
                  {compact ? null : <td className="tnum py-2.5 text-ink-3">{day(s.ended ?? s.started)}</td>}
                </tr>
              ))}
            </tbody>
          </table>
        ) : null}
        {rows.length > shown ? (
          <button type="button" className={`${btn} self-start`} onClick={() => setShown(shown + PAGE)}>
            {t.showMore(Math.min(PAGE, rows.length - shown))}
          </button>
        ) : null}
        <p className="m-0 text-[12.5px] text-ink-3">{t.sessionsNote}</p>
      </section>
      {selected ? (
        <SheetSteps ids={rows.slice(0, shown).map((r) => r.id)} current={selectedId} go={setSelectedId}>
          <SessionSheet key={selected.id} session={selected} onClose={() => setSelectedId(null)} />
        </SheetSteps>
      ) : null}
    </div>
  );
}

function SessionSheet({ session: s, onClose }: { session: Session; onClose: () => void }) {
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setNote(null);
    setError(null);
  }, [s.id]);
  const act = async (f: () => Promise<unknown>, done: string | null) => {
    setError(null);
    try {
      await f();
      setNote(done);
    } catch (e) {
      setError(getErrorMessage(e, t.loadFailed));
    }
  };
  return (
    <DetailSheet title={s.title} onClose={onClose} width={480}>
      <h2 className="m-0 break-words text-[22px] font-bold leading-tight tracking-[-0.03em]">{s.title}</h2>
      <p className="mt-2 flex items-center gap-2 text-[13px] text-ink-3">
        <span className="font-semibold text-ink-2">{t.origin[s.origin]}</span>
        {s.project ? (
          <span className="flex min-w-0 items-center gap-1">
            · <PathLink path={s.project}>{tildePath(s.project)}</PathLink>
          </span>
        ) : null}
      </p>
      <dl className="mt-4 grid grid-cols-3 gap-4">
        <Fact label={t.started} value={day(s.started)} />
        <Fact label={t.lastActive} value={day(s.ended)} />
        <Fact label={t.prompts} value={formatCount(s.prompts)} />
      </dl>
      <div className="mt-5 flex flex-wrap items-center gap-2">
        {s.resume ? (
          <>
            <button type="button" className={primary} onClick={() => void act(() => ipc.sessionResume(s.id), t.resumed)}>
              {t.resume}
            </button>
            <button type="button" className={btn} onClick={() => void act(() => navigator.clipboard.writeText(s.resume!), t.copied)}>
              {t.copyCommand}
            </button>
          </>
        ) : s.origin === "cursor_editor" && s.project ? (
          <button type="button" className={primary} onClick={() => void act(() => ipc.sessionOpenProject(s.id), null)}>
            {t.openInCursor}
          </button>
        ) : null}
        <button type="button" onClick={() => void ipc.revealInFinder(s.source)} className="ml-auto text-[12.5px] font-semibold text-ink-2 hover:text-ink">
          {t.showInFinder}
        </button>
      </div>
      {note ? <p className="mb-0 mt-2 text-[12.5px] text-ink-2">{note}</p> : null}
      {error ? (
        <p role="alert" className="mb-0 mt-2 text-[12.5px] text-danger">
          {error}
        </p>
      ) : null}
      {s.resume ? <div className="selectable mt-3 break-all rounded-input bg-panel px-3 py-2 font-mono text-[11.5px] text-ink-2">{s.resume}</div> : null}
      {s.origin === "cursor_editor" ? <p className="mt-3 text-[12.5px] text-ink-3">{t.cursorEditorNote}</p> : null}
      {s.origin === "gemini" ? <p className="mt-3 text-[12.5px] text-ink-3">{t.geminiNote}</p> : null}
      {s.first_prompt ? (
        <div className="mt-6">
          <h3 className="mb-2 mt-0 text-[12.5px] font-semibold text-ink-3">{t.firstPrompt}</h3>
          <p className="selectable m-0 whitespace-pre-wrap break-words text-[13.5px] leading-relaxed text-ink">{s.first_prompt}</p>
        </div>
      ) : null}
      <div className="selectable mt-6 truncate font-mono text-[11.5px] text-ink-3" title={s.source}>
        {tildePath(s.source)}
      </div>
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
