import { useEffect, useRef, useState } from "react";

/** Danger pill that fills left to right over 800 ms while held; release cancels (DESIGN.md §4). */
export function HoldButton({ label, onDone }: { label: string; onDone: () => void }) {
  const [holding, setHolding] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const start = () => {
    setHolding(true);
    timer.current = window.setTimeout(() => {
      setHolding(false);
      onDone();
    }, 800);
  };
  const stop = () => {
    window.clearTimeout(timer.current);
    setHolding(false);
  };
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return (
    <button
      type="button"
      onPointerDown={start}
      onPointerUp={stop}
      onPointerLeave={stop}
      onKeyDown={(e) => (e.key === " " || e.key === "Enter") && !e.repeat && start()}
      onKeyUp={stop}
      className="relative h-10 overflow-hidden rounded-full bg-danger-soft px-5 font-semibold text-danger"
    >
      <span
        aria-hidden="true"
        className="absolute inset-y-0 left-0 bg-danger/25"
        style={{ width: holding ? "100%" : "0%", transition: holding ? "width 800ms linear" : "width 120ms ease-out" }}
      />
      <span className="relative">{label}</span>
    </button>
  );
}
