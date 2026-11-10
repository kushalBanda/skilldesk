import { EASE } from "../lib/motion";

/** On/off switch (`.tg`, DESIGN.md update 2026-11-07 b): accent on, grey off, 44px hit area. */
export function Switch({ on, label, onChange, disabled }: { on: boolean; label: string; onChange: (next: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!on);
      }}
      className={[
        "group relative h-6 w-10 shrink-0 rounded-full transition-colors duration-150 disabled:opacity-40 motion-reduce:transition-none",
        on ? "bg-accent" : "bg-rule",
      ].join(" ")}
    >
      {/* The thumb stretches toward its next side on press, so feedback starts on pointer-down. */}
      <span
        style={{ transitionTimingFunction: EASE }}
        className={[
          "absolute top-0.5 h-5 w-5 rounded-full bg-white shadow-card transition-[left,width] duration-200 group-enabled:group-active:w-6 motion-reduce:transition-none",
          on ? "left-[18px] group-enabled:group-active:left-[14px]" : "left-0.5",
        ].join(" ")}
      />
    </button>
  );
}
