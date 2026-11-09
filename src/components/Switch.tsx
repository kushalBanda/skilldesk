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
        "relative h-6 w-10 shrink-0 rounded-full transition-colors duration-150 disabled:opacity-40",
        on ? "bg-accent" : "bg-rule",
      ].join(" ")}
    >
      <span className={["absolute top-0.5 h-5 w-5 rounded-full bg-white shadow-card transition-[left] duration-150", on ? "left-[18px]" : "left-0.5"].join(" ")} />
    </button>
  );
}
