import type { ReactNode } from "react";

/** Filter chip (DESIGN.md "Filter row"): label · count. Selected = grey fill + ink ring. */
export function FChip({
  label,
  icon,
  count,
  active,
  onClick,
}: {
  label: string;
  /** Agent mark or UI glyph before the label (DESIGN.md "Filter row"). */
  icon?: ReactNode;
  count?: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={[
        "inline-flex h-8 items-center gap-1.5 rounded-full px-3 text-[12.5px] font-medium transition-transform duration-100 active:scale-[0.97]",
        active ? "bg-sunk text-ink ring-1 ring-ink" : "bg-bg text-ink-2 ring-1 ring-rule hover:bg-sunk",
      ].join(" ")}
    >
      {icon}
      <span>{label}</span>
      {count !== undefined ? <span className="tnum text-ink-3">{count}</span> : null}
    </button>
  );
}
