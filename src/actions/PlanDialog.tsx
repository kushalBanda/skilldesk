import { useEffect } from "react";
import type { ActionPlan } from "../ipc/bindings/ActionPlan";
import type { FileOp } from "../ipc/bindings/FileOp";
import { useOverview } from "../ipc/queries";
import { agentName } from "../lib/agents";
import { formatBytes, formatCount, tildePath } from "../lib/format";
import { t } from "../strings";

const isTrash = (p: string) => /\/\.Trash\//.test(p);
const isStaging = (p: string) => /\/\.skilldesk\/staging\//.test(p);

function describe(op: FileOp): string {
  switch (op.op) {
    case "create_symlink":
      return `${t.opLink} ${tildePath(op.link)} → ${tildePath(op.to)}`;
    case "remove_symlink":
      return `${t.opUnlink} ${tildePath(op.link)}`;
    case "move_path":
      if (isStaging(op.from)) return `${t.opCopyInto} ${tildePath(op.to)}`;
      if (isTrash(op.to)) return `${t.opTrash} ${tildePath(op.from)}`;
      return `${t.opMove} ${tildePath(op.from)} → ${tildePath(op.to)}`;
    case "unlink_dangling":
      return `${t.opUnlink} ${tildePath(op.link)}`;
    case "relink_dangling":
      return `${t.opLink} ${tildePath(op.link)} → ${tildePath(op.to)}`;
    case "edit_text":
      return `${t.opEdit} ${tildePath(op.path)}`;
    case "mask_text":
      return `${t.opMask(op.spans.length)} ${tildePath(op.path)}`;
    case "unmask_text":
      return `${t.opRestore} ${tildePath(op.path)}`;
  }
}

function folderName(p: string): string {
  return `${p.replace(/\/+$/, "").split("/").pop() ?? p}/`;
}

/** Shown only when a change reaches beyond the clicked agent, moves a real
 *  folder, or deletes files (then as Replace preview, DESIGN.md §7.16). */
export function PlanDialog({ plan, onCancel, onConfirm }: { plan: ActionPlan; onCancel: () => void; onConfirm: () => void }) {
  const agents = useOverview().data?.agents ?? [];
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);
  const replacing = plan.removed.length > 0;
  const then = replacing ? plan.ops.filter((op) => !(op.op === "move_path" && isTrash(op.to))) : plan.ops;
  const unlinkShared = plan.request.type === "skill_toggle" && !plan.request.on && plan.agents.length > 1;
  const cta = replacing ? t.trashAndLink : unlinkShared ? t.unlinkFor(plan.agents.length) : t.apply;
  return (
    <div className="scrim-host fixed inset-0 z-40 flex items-center justify-center" onClick={onCancel}>
      <div className="scrim absolute inset-0" data-motion aria-hidden="true" />
      <div
        role="dialog"
        data-motion
        aria-modal="true"
        aria-labelledby="plan-title"
        className="palette-in relative flex max-h-[88vh] w-[560px] max-w-[90vw] flex-col rounded-panel bg-panel p-6 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 id="plan-title" className="m-0 text-[22px] font-bold tracking-[-0.03em]">
          {replacing ? t.replaceTitle(plan.removed.length, folderName(plan.removed[0]!.path).replace(/\/$/, "")) : plan.summary}
        </h2>
        {plan.agents.length > 1 ? (
          <p className="mt-2 text-ink-2">
            {t.affects(plan.agents.length)} {plan.agents.map((a) => agentName(agents, a)).join(", ")}.
          </p>
        ) : null}
        {replacing ? (
          <div className="mt-4 overflow-hidden rounded-row">
            <div className="bg-danger-soft px-3 py-2 text-[13px] font-semibold text-danger">{t.willBeRemoved}</div>
            <ul className="selectable m-0 flex list-none flex-col gap-1 bg-sunk p-3 font-mono text-[12.5px]">
              {plan.removed.map((r) => (
                <li key={r.path} className="flex gap-3">
                  <span className="min-w-0 flex-1 truncate" title={r.path}>
                    {tildePath(r.path.replace(/[^/]+\/?$/, ""))}
                    <b>{folderName(r.path)}</b>
                  </span>
                  <span className="shrink-0 text-ink-3">
                    {formatCount(r.files)} {r.files === 1 ? t.file : t.files} · {formatBytes(r.bytes)}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
        {replacing ? <h3 className="mb-1 mt-4 text-[12.5px] font-semibold text-ink-3">{t.then}</h3> : null}
        <ul className={["selectable m-0 flex min-h-0 shrink list-none flex-col gap-1 overflow-auto rounded-row bg-sunk p-3 font-mono text-[12.5px]", replacing ? "mt-0" : "mt-4"].join(" ")}>
          {then.map((op, i) => (
            <li key={i} className="break-all">
              {describe(op)}
            </li>
          ))}
        </ul>
        {plan.untouched.length ? (
          <div className="mt-4">
            <h3 className="mb-1 mt-0 text-[12.5px] font-semibold text-ink-3">{t.notChanged}</h3>
            <ul className="selectable m-0 flex max-h-40 list-none flex-col gap-1 overflow-auto rounded-row bg-sunk p-3 text-[12.5px]">
              {plan.untouched.map((u, i) => (
                <li key={i}>
                  <span className="font-semibold">{t.placeName[u.place] ?? u.place}</span>
                  {u.chat ? <span> · {u.chat}</span> : null}
                  <span className="block font-mono text-ink-3">{tildePath(u.path)}</span>
                  <span className="block text-ink-2">{u.reason}</span>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
        <p className="mt-3 text-[13px] text-ink-2">{plan.request.type === "secret_redact" ? t.redactUndoNote : plan.request.type === "storage_trash" ? t.cleanupUndoNote : replacing ? t.trashUndoNote : t.undoNote}</p>
        <div className="mt-5 flex justify-end gap-2">
          <button type="button" onClick={onCancel} className="h-10 rounded-full bg-bg px-5 font-semibold ring-1 ring-rule active:scale-[0.97]">
            {t.cancel}
          </button>
          <button
            type="button"
            autoFocus
            onClick={onConfirm}
            className={["h-10 rounded-full px-5 font-semibold active:scale-[0.97]", replacing ? "bg-danger text-white" : "bg-ink text-bg"].join(" ")}
          >
            {cta}
          </button>
        </div>
      </div>
    </div>
  );
}
