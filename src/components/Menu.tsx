// Pop-up menu (replaces the native <select>): grows from its trigger, checkmark
// on the current choice, ↑↓ / Home / End / ↩ / Esc, closes on outside click.
import { useEffect, useId, useRef, useState, type ReactNode } from "react";

export type MenuOption<V> = { value: V; label: string; hint?: string };

export function Menu<V extends string | number>({
  value,
  options,
  onChange,
  label,
  icon,
}: {
  value: V;
  options: MenuOption<V>[];
  onChange: (v: V) => void;
  /** Accessible name of the trigger ("Date range"). */
  label: string;
  icon?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const current = options.find((o) => o.value === value) ?? options[0]!;

  const show = () => {
    setActive(Math.max(0, options.findIndex((o) => o.value === value)));
    setOpen(true);
  };
  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) trigger.current?.focus();
  };
  const pick = (i: number) => {
    const o = options[i];
    if (o) onChange(o.value);
    close();
  };

  useEffect(() => {
    if (!open) return;
    list.current?.focus();
    const away = (e: PointerEvent) => !root.current?.contains(e.target as Node) && close(false);
    window.addEventListener("pointerdown", away);
    return () => window.removeEventListener("pointerdown", away);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // Long lists scroll: keep the active option in view while arrowing.
  useEffect(() => {
    if (open) document.getElementById(`${id}-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [open, active, id]);

  return (
    <div ref={root} className="relative inline-flex">
      <button
        ref={trigger}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={id}
        aria-label={`${label}: ${current.label}`}
        onClick={() => (open ? close() : show())}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            show();
          }
        }}
        className={[
          "inline-flex h-8 items-center gap-2 rounded-full pl-3 pr-2.5 text-[12.5px] font-medium text-ink ring-1 transition-[background-color,transform] duration-100 active:scale-[0.97]",
          open ? "bg-sunk ring-rule" : "bg-sunk ring-transparent hover:ring-rule",
        ].join(" ")}
      >
        {icon}
        <span>{current.label}</span>
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" className={["text-ink-3 transition-transform duration-150", open ? "rotate-180" : ""].join(" ")}>
          <path d="M3 4.5L6 7.5l3-3" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>
      {open ? (
        <ul
          ref={list}
          id={id}
          role="listbox"
          aria-label={label}
          aria-activedescendant={`${id}-${active}`}
          tabIndex={-1}
          data-motion
          onKeyDown={(e) => {
            if (e.key === "Escape" || e.key === "Tab") {
              e.preventDefault();
              e.stopPropagation();
              close();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((i) => Math.min(options.length - 1, i + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((i) => Math.max(0, i - 1));
            } else if (e.key === "Home") {
              e.preventDefault();
              setActive(0);
            } else if (e.key === "End") {
              e.preventDefault();
              setActive(options.length - 1);
            } else if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              pick(active);
            }
          }}
          className="glass menu-in absolute left-0 top-[calc(100%+6px)] z-30 m-0 max-h-[min(420px,60vh)] min-w-[240px] max-w-[340px] overflow-auto list-none rounded-row p-1.5 shadow-card ring-1 ring-rule outline-none"
        >
          {options.map((o, i) => {
            const on = o.value === value;
            return (
              <li
                key={String(o.value)}
                id={`${id}-${i}`}
                role="option"
                aria-selected={on}
                onPointerMove={() => i !== active && setActive(i)}
                onClick={() => pick(i)}
                className={["flex h-9 items-center gap-2.5 rounded-input px-2.5 text-[13px]", i === active ? "bg-accent-soft text-ink" : "text-ink-2"].join(" ")}
              >
                <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true" className={on ? "text-accent" : "invisible"}>
                  <path d="M3 7.5l2.5 2.5L11 4.5" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
                <span className={["min-w-0 flex-1 truncate", on ? "font-semibold text-ink" : ""].join(" ")} title={o.label}>{o.label}</span>
                {o.hint ? <span className="tnum text-[12px] text-ink-3">{o.hint}</span> : null}
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}
