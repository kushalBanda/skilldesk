import { BackLink } from "../components/BackLink";
// Adopt into library (DESIGN.md §7.15): one skill at a time from those in
// 2+ places. Places → pick the copy to keep (diff) → result. The write goes
// through the action engine with Replace preview.
import { useEffect, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { getErrorMessage } from "../ipc/error";
import { useItems, useOverview } from "../ipc/queries";
import { useAction } from "../actions/ActionProvider";
import type { Item } from "../ipc/bindings/Item";
import type { CopyInfo } from "../ipc/bindings/CopyInfo";
import type { DiffRow } from "../ipc/bindings/DiffRow";
import { agentName } from "../lib/agents";
import { formatCount, tildePath } from "../lib/format";
import { t } from "../strings";

function places(i: Item): number {
  if (i.facts.kind !== "skill") return 0;
  const f = i.facts;
  const own = f.locations.filter((l) => l.link.kind === "real_dir" && !l.plugin && !l.project).length;
  return own + (f.library ? 1 : 0);
}

export function Adopt() {
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const skills = useItems("skill").data ?? [];
  const list = useMemo(
    () =>
      skills
        .map((s) => ({ s, n: places(s) }))
        .filter((x) => x.n >= 2)
        .sort((a, b) => b.n - a.n || a.s.name.localeCompare(b.s.name))
        .map((x) => x.s),
    [skills],
  );
  const id = params.get("id") ?? list[0]?.id ?? null;
  const idx = list.findIndex((s) => s.id === id);
  const go = (i: number) => list[i] && setParams({ id: list[i]!.id });

  return (
    <div className="h-full overflow-auto page">
      <div className="mb-3">
        <BackLink fallback={{ to: "/skills", label: t.skills }} />
      </div>
      <div className="flex items-end justify-between gap-4">
        <h1 className="m-0 text-[44px] font-bold leading-none tracking-[-0.05em]">{t.adoptTitle}</h1>
        {list.length ? (
          <div className="flex items-center gap-2 text-[13px] text-ink-2">
            <button
              type="button"
              aria-label={t.previous}
              disabled={idx <= 0}
              onClick={() => go(idx - 1)}
              className="flex h-8 w-8 items-center justify-center rounded-input hover:bg-sunk disabled:opacity-30"
            >
              ‹
            </button>
            <span className="tnum">{t.pager(Math.max(idx, 0) + 1, list.length)}</span>
            <button
              type="button"
              aria-label={t.next}
              disabled={idx < 0 || idx >= list.length - 1}
              onClick={() => go(idx + 1)}
              className="flex h-8 w-8 items-center justify-center rounded-input hover:bg-sunk disabled:opacity-30"
            >
              ›
            </button>
          </div>
        ) : null}
      </div>
      {id ? (
        <AdoptOne key={id} id={id} onDone={() => (idx >= 0 && idx < list.length - 1 ? go(idx + 1) : navigate("/skills?status=library"))} />
      ) : (
        <p className="mt-6 text-ink-2">{t.adoptNone}</p>
      )}
    </div>
  );
}

function AdoptOne({ id, onDone }: { id: string; onDone: () => void }) {
  const agents = useOverview().data?.agents ?? [];
  const { run } = useAction();
  const view = useQuery({ queryKey: ["adopt", id], queryFn: () => ipc.adoptView(id), retry: false });
  const v = view.data;
  const [keep, setKeep] = useState<string | null>(null);
  const [other, setOther] = useState<string | null>(null);
  useEffect(() => {
    setKeep(null);
    setOther(null);
  }, [id]);
  const keeper = keep ?? v?.keep ?? null;
  const compare = other ?? v?.copies.find((c) => c.path !== keeper && c.differs > 0)?.path ?? null;
  const diff = useQuery({
    queryKey: ["skill-diff", id, keeper, compare],
    queryFn: () => ipc.skillDiff(id, keeper!, compare!),
    enabled: !!keeper && !!compare,
  });
  if (view.error)
    return (
      <p role="alert" className="mt-6 text-danger">
        {getErrorMessage(view.error, t.loadFailed)}
      </p>
    );
  if (!v) return null;

  const names = (c: { agents: string[] }) => c.agents.map((a) => agentName(agents, a)).join(", ");
  const keptCopy = v.copies.find((c) => c.path === keeper);
  const differ = v.copies.filter((c) => c.group !== keptCopy?.group);
  const lineCounts = new Set(v.copies.map((c) => c.lines));
  const libPath = v.copies.find((c) => c.in_library)?.path ?? `~/.skilldesk/skills/${v.name}`;
  const toTrash = v.copies.length - 1;
  const label = (c: CopyInfo) => (c.in_library ? t.inLibrary : names(c) || tildePath(c.path));

  return (
    <>
      <h2 className="mb-1 mt-6 text-[28px] font-bold tracking-[-0.03em]">{v.name}</h2>
      <p className="m-0 text-ink-2">
        {t.placesHold(v.copies.length + v.links.length, v.name)} · {t.nMatch(v.copies.length - differ.length)}
        {differ.length ? ` · ${t.nDiffer(differ.length)}` : ""}
        {lineCounts.size === 1 ? ` · ${t.linesEach([...lineCounts][0]!)}` : ""}
      </p>

      <h3 className="mb-2 mt-8 text-[15px] font-semibold">1. {t.places}</h3>
      <ul className="m-0 flex max-w-[860px] list-none flex-col gap-2 p-0">
        {v.copies.map((c) => {
          const same = c.group === keptCopy?.group;
          return (
            <li key={c.path}>
              <button
                type="button"
                onClick={() => c.path !== keeper && setOther(c.path)}
                className={[
                  "flex w-full items-center gap-3 rounded-row px-3 py-2.5 text-left",
                  c.path === compare ? "bg-accent-soft" : "bg-panel hover:bg-sunk",
                ].join(" ")}
              >
                <span className="min-w-0 flex-1">
                  <span className="block font-semibold">{label(c)}</span>
                  <span className="selectable block truncate font-mono text-[12px] text-ink-3" title={c.path}>
                    {tildePath(c.path)}
                  </span>
                </span>
                {c.path === keeper ? <span className="rounded-full bg-ink px-2 py-0.5 text-[11.5px] font-medium text-bg">{t.keeping}</span> : null}
                <span
                  className={["rounded-full px-2 py-0.5 text-[11.5px] font-medium", same ? "bg-[var(--ok-soft)] text-ok" : "bg-sunk text-warn-text"].join(" ")}
                >
                  {same ? t.matches : t.differsBy(c.differs)}
                </span>
              </button>
            </li>
          );
        })}
        {v.links.map((l) => (
          <li key={l.path} className="flex items-center gap-3 rounded-row px-3 py-2 text-[13px] text-ink-2">
            <span className="min-w-0 flex-1 truncate">
              <span className="font-semibold text-ink">{names(l)}</span> · {t.linkTo} {tildePath(l.to)}
            </span>
          </li>
        ))}
      </ul>

      <h3 className="mb-2 mt-8 text-[15px] font-semibold">2. {t.pickCopy}</h3>
      {!compare ? (
        <p className="m-0 text-ink-2">{t.allMatch(keptCopy ? label(keptCopy) : "")}</p>
      ) : (
        <div className="max-w-[1100px] overflow-hidden rounded-panel bg-panel">
          <div className="grid grid-cols-2 border-b border-rule">
            {[keeper!, compare].map((p) => {
              const c = v.copies.find((x) => x.path === p)!;
              return (
                <label key={p} className="flex cursor-default items-center gap-2 px-3 py-2.5">
                  <input type="radio" name="keep" checked={keeper === p} onChange={() => (setOther(keeper), setKeep(p))} className="accent-[var(--accent)]" />
                  <span className="min-w-0">
                    <span className="block text-[13px] font-semibold">
                      {t.keepThis} · {label(c)}
                    </span>
                    <span className="block truncate font-mono text-[11.5px] text-ink-3">{tildePath(p)}</span>
                  </span>
                </label>
              );
            })}
          </div>
          <DiffTable rows={diff.data?.rows ?? []} />
          {diff.data?.other_files.length ? (
            <p className="m-0 border-t border-rule px-3 py-2 text-[12.5px] text-warn-text">
              {t.otherFilesDiffer} {diff.data.other_files.join(", ")}
            </p>
          ) : null}
        </div>
      )}

      <h3 className="mb-2 mt-8 text-[15px] font-semibold">3. {t.result}</h3>
      <p className="m-0 max-w-[860px] text-ink-2">{t.adoptResult(tildePath(libPath), v.copies.length - 1 + v.links.length, toTrash)}</p>
      <button
        type="button"
        disabled={!keeper}
        onClick={() => void run({ type: "library_adopt", id, keep: keeper! }, onDone)}
        className="mt-4 h-11 rounded-full bg-ink px-6 font-semibold text-bg active:scale-[0.98]"
      >
        {t.adoptCta}
      </button>
    </>
  );
}

function DiffTable({ rows }: { rows: DiffRow[] }) {
  return (
    <div className="selectable font-mono text-[12px] leading-[1.55]">
      {rows.map((r, i) =>
        r.kind === "same" ? (
          <div key={i} className="bg-sunk px-3 py-1 text-center text-[11.5px] text-ink-3">
            {r.from === r.to ? t.lineMatches(r.from) : t.linesMatch(r.from, r.to)}
          </div>
        ) : (
          <div key={i} className="grid grid-cols-2">
            <Cell side={r.left} tone="bg-danger-soft" />
            <Cell side={r.right} tone="bg-[var(--ok-soft)]" />
          </div>
        ),
      )}
    </div>
  );
}

function Cell({ side, tone }: { side: [number, string] | null; tone: string }) {
  if (!side) return <div className="bg-sunk/50" />;
  return (
    <div className={["flex gap-2 px-3 py-0.5 text-ink", tone].join(" ")}>
      <span className="tnum w-8 shrink-0 text-right text-ink-3">{formatCount(side[0])}</span>
      <span className="min-w-0 whitespace-pre-wrap break-all">{side[1] || " "}</span>
    </div>
  );
}
