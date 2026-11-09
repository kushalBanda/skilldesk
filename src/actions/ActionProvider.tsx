// One place that runs every change: plan → (confirm when it reaches further
// than the click) → apply → Undo toast for 10 s (DESIGN.md principle 3).
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { getErrorMessage, isAppError } from "../ipc/error";
import type { ActionPlan } from "../ipc/bindings/ActionPlan";
import type { ActionRequest } from "../ipc/bindings/ActionRequest";
import { PlanDialog } from "./PlanDialog";
import { t } from "../strings";
import { departure, leave, settle, snapshot, toHistory } from "../lib/motion";

type Toast = { text: string; undoId?: string; error?: boolean; key: number };
const TOAST_MS = 10_000;
/** `onApplied` runs once the change is written (after the confirm dialog when there is one). */
type Ctx = { run: (req: ActionRequest, onApplied?: () => void) => Promise<void>; undo: (id: string) => Promise<void> };

const ActionCtx = createContext<Ctx | null>(null);

export function useAction(): Ctx {
  const ctx = useContext(ActionCtx);
  if (!ctx) throw new Error("useAction outside ActionProvider");
  return ctx;
}

function messageOf(err: unknown): string {
  if (isAppError(err) && err.kind === "target_conflict" && err.details?.conflicts.length) {
    const c = err.details.conflicts[0]!;
    return `${t.nothingChanged} ${c.path.replace(/^\/Users\/[^/]+/, "~")} ${c.reason}.`;
  }
  return getErrorMessage(err, t.actionFailed);
}

export function ActionProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<{ plan: ActionPlan; onApplied?: () => void } | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const client = useQueryClient();
  // Our own writes refresh everything they could touch; events cover outside changes.
  const refresh = useCallback(() => client.invalidateQueries(), [client]);

  const [closing, setClosing] = useState(false);
  const left = useRef(TOAST_MS);
  const started = useRef(0);
  const close = useCallback(() => {
    window.clearTimeout(timer.current);
    setClosing(true);
    timer.current = window.setTimeout(() => {
      setToast(null);
      setClosing(false);
    }, 180);
  }, []);
  const arm = useCallback(
    (ms: number) => {
      window.clearTimeout(timer.current);
      left.current = ms;
      started.current = performance.now();
      timer.current = window.setTimeout(close, ms);
    },
    [close],
  );
  const show = useCallback(
    (next: Omit<Toast, "key">) => {
      setClosing(false);
      setToast({ ...next, key: Date.now() });
      arm(TOAST_MS);
    },
    [arm],
  );
  // Hovering the toast holds it (and its Undo) until the pointer leaves.
  const hold = () => {
    window.clearTimeout(timer.current);
    left.current = Math.max(1200, left.current - (performance.now() - started.current));
  };
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const apply = useCallback(
    async (plan: ActionPlan, onApplied?: () => void) => {
      // Rows start leaving while the write runs; a failure brings them back.
      const gone = departure(plan.request);
      const before = snapshot();
      const going = gone ? leave(gone.ids, gone.how) : null;
      try {
        const rec = await ipc.actionApply(plan.id);
        await going?.done;
        if (gone?.how === "trash") toHistory(going?.from ?? null, t.toHistory);
        onApplied?.();
        await refresh();
        requestAnimationFrame(() => settle(before));
        show({ text: rec.summary, undoId: rec.id });
      } catch (err) {
        going?.cancel();
        show({ text: messageOf(err), error: true });
      }
    },
    [refresh, show],
  );

  const run = useCallback(
    async (req: ActionRequest, onApplied?: () => void) => {
      try {
        const plan = await ipc.actionPlan(req);
        if (plan.needs_confirm) setPending({ plan, onApplied });
        else await apply(plan, onApplied);
      } catch (err) {
        show({ text: messageOf(err), error: true });
      }
    },
    [apply, show],
  );

  const undo = useCallback(
    async (id: string) => {
      try {
        const before = snapshot();
        const rec = await ipc.actionUndo(id);
        await refresh();
        // What came back fades in where it was, with a soft highlight.
        requestAnimationFrame(() => settle(before));
        show({ text: `${t.undone}: ${rec.summary}` });
      } catch (err) {
        await refresh();
        show({ text: messageOf(err), error: true });
      }
    },
    [refresh, show],
  );

  return (
    <ActionCtx.Provider value={{ run, undo }}>
      {children}
      {pending ? (
        <PlanDialog
          plan={pending.plan}
          onCancel={() => setPending(null)}
          onConfirm={() => {
            const p = pending;
            setPending(null);
            void apply(p.plan, p.onApplied);
          }}
        />
      ) : null}
      {toast ? (
        <div
          key={toast.key}
          role="status"
          data-motion
          onPointerEnter={hold}
          onPointerLeave={() => arm(left.current)}
          className={[
            "fixed bottom-6 left-1/2 z-50 flex max-w-[640px] -translate-x-1/2 items-center gap-4 overflow-hidden rounded-full py-2.5 pl-5 pr-2 text-[13px] shadow-card",
            toast.error ? "bg-danger text-white" : "bg-ink text-bg",
            closing ? "toast-out" : "toast-in",
          ].join(" ")}
        >
          <span className="truncate">{toast.text}</span>
          {toast.undoId ? (
            <button
              type="button"
              className="h-7 shrink-0 rounded-full px-3 font-semibold transition-[background-color,transform] duration-100 hover:bg-[color-mix(in_srgb,currentColor_14%,transparent)] active:scale-[0.95]"
              onClick={() => {
                const id = toast.undoId!;
                close();
                void undo(id);
              }}
            >
              {t.undo}
            </button>
          ) : (
            <span className="w-3" />
          )}
          {/* Time left for Undo; stops while the pointer is on the toast. */}
          {toast.undoId ? <i aria-hidden="true" className="toast-clock pointer-events-none absolute inset-x-5 bottom-0 h-px origin-left bg-current opacity-30" /> : null}
        </div>
      ) : null}
    </ActionCtx.Provider>
  );
}
