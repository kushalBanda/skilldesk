import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { t } from "../strings";
import { justArrived, pulse } from "../lib/motion";

/** The list a sheet steps through: the screen's rows in the order shown. */
type Steps = { ids: string[]; current: string | null; go: (id: string) => void };
const StepCtx = createContext<Steps | null>(null);

/** Wrap a screen's sheet so its header can show previous / next (DESIGN.md §3). */
export function SheetSteps({ ids, current, go, children }: Steps & { children: ReactNode }) {
  return <StepCtx.Provider value={{ ids, current, go }}>{children}</StepCtx.Provider>;
}

const SHEET_MS = 240;
// A sheet that replaces another (previous / next remounts some sheets) does not slide in again.
let lastUnmount = 0;

const selectedRow = () => document.querySelector("main section .bg-accent-soft:is(tr, li, button, a, [role='row'], [role='option'])");

/** Keep the selected row on screen (every list marks it with --accent-soft). */
function revealRow() {
  selectedRow()?.scrollIntoView({ block: "nearest" });
}

function editable(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  return el.isContentEditable || el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT";
}

/** Right sheet, no scrim (parallel task). Esc and » close; ↑ ↓ step through the list (DESIGN.md §3, §5). */
export function DetailSheet({
  title,
  width = 380,
  onClose,
  children,
}: {
  title: string;
  width?: number;
  onClose: () => void;
  children: ReactNode;
}) {
  const steps = useContext(StepCtx);
  const [enter] = useState(() => Date.now() - lastUnmount > 120);
  const [leaving, setLeaving] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const body = useRef<HTMLDivElement>(null);
  useEffect(
    () => () => {
      window.clearTimeout(timer.current);
      lastUnmount = Date.now();
    },
    [],
  );

  const close = useCallback(() => {
    if (timer.current !== undefined) return;
    setLeaving(true);
    timer.current = window.setTimeout(() => {
      timer.current = undefined;
      setLeaving(false);
      onClose();
    }, SHEET_MS - 60);
  }, [onClose]);

  // Opened from a deep link (⌘K, Fix list): bring its row into view once and
  // pulse it, so the eye lands on what the link was about.
  const hasSteps = steps !== null;
  useEffect(() => {
    if (!hasSteps) return;
    requestAnimationFrame(() => {
      const el = selectedRow();
      if (!el) return;
      const r = el.getBoundingClientRect();
      const off = r.top < 0 || r.bottom > window.innerHeight;
      el.scrollIntoView({ block: "nearest", behavior: off ? "smooth" : "auto" });
      if (off || justArrived()) pulse(el);
    });
  }, [hasSteps]);

  const index = steps?.current ? steps.ids.indexOf(steps.current) : -1;
  const prev = steps && index > 0 ? steps.ids[index - 1]! : null;
  const next = steps && index >= 0 && index < steps.ids.length - 1 ? steps.ids[index + 1]! : null;
  const go = useCallback(
    (id: string | null) => {
      if (!id || !steps) return;
      // Focus left on the old row would ring the wrong row; the sheet keeps its own.
      const el = document.activeElement;
      if (el instanceof HTMLElement && !body.current?.parentElement?.contains(el)) el.blur();
      steps.go(id);
      body.current?.scrollTo({ top: 0 });
      // Keep the selected row on screen (every list marks it with --accent-soft).
      requestAnimationFrame(revealRow);
    },
    [steps],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // A dialog or the palette on top owns the keys.
      if (document.querySelector("[aria-modal='true']")) return;
      if (e.key === "Escape") return close();
      if (e.metaKey || e.ctrlKey || e.altKey || editable(e.target)) return;
      if (e.key === "ArrowDown" && next) {
        e.preventDefault();
        go(next);
      } else if (e.key === "ArrowUp" && prev) {
        e.preventDefault();
        go(prev);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close, go, next, prev]);

  return (
    <aside
      aria-label={title}
      style={{ width }}
      data-motion
      className={[
        "flex h-full shrink-0 flex-col overflow-hidden border-l border-rule bg-bg",
        leaving ? "sheet-out" : enter ? "sheet-in" : "",
      ].join(" ")}
    >
      <header className="flex h-12 shrink-0 items-center gap-1 px-4">
        <IconBtn label={t.close} onClick={close}>
          <path d="M4 4l4 4-4 4M8.5 4l4 4-4 4" />
        </IconBtn>
        {steps && index >= 0 ? (
          <>
            <IconBtn label={t.previous} title={`${t.previous} (↑)`} disabled={!prev} onClick={() => go(prev)}>
              <path d="M4 10l4-4 4 4" />
            </IconBtn>
            <IconBtn label={t.next} title={`${t.next} (↓)`} disabled={!next} onClick={() => go(next)}>
              <path d="M4 6l4 4 4-4" />
            </IconBtn>
            <span className="tnum ml-1 text-[12px] text-ink-3">{t.nOfM(index + 1, steps.ids.length)}</span>
          </>
        ) : null}
      </header>
      <div ref={body} className="flex-1 overflow-auto px-6 pb-8">
        {children}
      </div>
    </aside>
  );
}

function IconBtn({
  label,
  title,
  disabled,
  onClick,
  children,
}: {
  label: string;
  title?: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      title={title ?? label}
      className="flex h-7 w-7 items-center justify-center rounded-input text-ink-2 hover:bg-sunk active:scale-[0.92] disabled:opacity-35 disabled:hover:bg-transparent"
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        {children}
      </svg>
    </button>
  );
}
