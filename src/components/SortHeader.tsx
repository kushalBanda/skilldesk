import { useEffect, useState, type CSSProperties, type PointerEvent as RPointerEvent } from "react";

export type Sort<K extends string> = { key: K; dir: "asc" | "desc" } | null;

/** Click a header: sorts by it (numbers and dates high → low first, names A → Z); again flips; third click clears. */
export function useSort<K extends string>(initial: Sort<K> = null) {
  // A list already ordered by a column says so: that header shows its arrow,
  // and the first click flips it (a click that re-applies the same order looks broken).
  const [sort, setSort] = useState<Sort<K>>(initial);
  const toggle = (key: K, first: "asc" | "desc") =>
    setSort((s) => (!s || s.key !== key ? { key, dir: first } : s.dir === first ? { key, dir: first === "asc" ? "desc" : "asc" } : null));
  return { sort, toggle };
}

export function sortBy<T, K extends string>(rows: T[], sort: Sort<K>, value: (row: T, key: K) => string | number | null): T[] {
  if (!sort) return rows;
  const sign = sort.dir === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const x = value(a, sort.key);
    const y = value(b, sort.key);
    // Rows with no value go last either way.
    if (x === null || x === "") return y === null || y === "" ? 0 : 1;
    if (y === null || y === "") return -1;
    return (
      (typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y), undefined, { sensitivity: "base", numeric: true })) * sign
    );
  });
}

/** Widths are shares of the table (0–1), so they follow the window's size. */
const MIN_PX = 56;
/** The column that takes what is left never goes under this. */
const FILL_MIN = 140;
const MAX_SHARE = 0.45;
const key = (table: string, k: string) => `sd.colshare.${table}.${k}`;

function savedWidth(table: string, k: string): number | null {
  try {
    const raw = localStorage.getItem(key(table, k));
    const n = raw === null ? NaN : Number(raw);
    return Number.isFinite(n) && n > 0 && n <= MAX_SHARE ? n : null;
  } catch {
    return null;
  }
}

const EVT = "sd:colw";
type WidthEvent = CustomEvent<{ table: string; k: string; w: number | null }>;

function saveWidth(table: string, k: string, w: number | null) {
  window.dispatchEvent(new CustomEvent(EVT, { detail: { table, k, w } }));
  try {
    if (w === null) localStorage.removeItem(key(table, k));
    else localStorage.setItem(key(table, k), w.toFixed(4));
  } catch {
    /* private mode: widths last for this session only */
  }
}

/**
 * The one table header cell (DESIGN.md "Tables"): every column sorts (click:
 * first direction, flip, clear) and every column but the last resizes from its
 * right edge. Dragging tracks the pointer 1:1; double-click returns the column
 * to its default; with the handle focused, ← → change it by 16px. Widths are
 * remembered per table as shares of its width (56px to 45%), so a narrower
 * window keeps every column visible.
 */
export function Th<K extends string>({
  table,
  label,
  k,
  first = "desc",
  sort,
  onSort,
  className = "",
  align = "left",
  resize = true,
  style,
}: {
  /** Names the table for remembered widths ("plugins"). */
  table: string;
  label: string;
  k: K;
  first?: "asc" | "desc";
  sort?: Sort<K>;
  onSort?: (k: K, first: "asc" | "desc") => void;
  className?: string;
  align?: "left" | "right";
  /** Off for the last column (it takes what is left). */
  resize?: boolean;
  style?: CSSProperties;
}) {
  // The one column without a fixed width class takes what is left; it never
  // gets a width of its own, so the table always has room to give and take.
  const fill = !/(^|\s)w-/.test(className);
  const [width, setWidth] = useState<number | null>(() => (fill ? null : savedWidth(table, k)));
  const [dragging, setDragging] = useState(false);
  // A column can be the flexible one in one layout (a sheet open) and not in another.
  useEffect(() => setWidth(fill ? null : savedWidth(table, k)), [fill, table, k]);
  const on = sort?.key === k;
  // A neighbour's drag can set this column's width (see `fill` below).
  useEffect(() => {
    const onW = (e: Event) => {
      const d = (e as WidthEvent).detail;
      if (d.table === table && d.k === k && !fill) setWidth(d.w);
    };
    window.addEventListener(EVT, onW);
    return () => window.removeEventListener(EVT, onW);
  }, [table, k, fill]);
  /** px to a share of `tableW`, between 56px and 45%. */
  const share = (px: number, tableW: number) => Math.max(MIN_PX / tableW, Math.min(MAX_SHARE, px / tableW));
  const startDrag = (e: RPointerEvent<HTMLSpanElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const th = e.currentTarget.parentElement as HTMLTableCellElement;
    // The border under the pointer moves 1:1: this column grows and the next
    // one gives up the same width. The column that fills the rest (no width of
    // its own) never gets one: when it is this column, only the next one
    // changes; when it is the next one, it absorbs the change by itself.
    const next = th.nextElementSibling as HTMLTableCellElement | null;
    const isFill = (el: HTMLElement | null) => !!el && el.dataset.fill === "1";
    const meFill = isFill(th);
    const nextK = next && next.dataset.col !== undefined && !isFill(next) ? next.dataset.col : null;
    const tableW = th.closest("table")!.getBoundingClientRect().width;
    const startW = th.getBoundingClientRect().width;
    const startNext = next?.getBoundingClientRect().width ?? 0;
    // Keep the offset from where the edge was grabbed.
    const startX = e.clientX;
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
    const move = (ev: PointerEvent) => {
      // Neither side goes under 56px.
      const lo = -(startW - MIN_PX);
      const hi = nextK ? startNext - MIN_PX : isFill(next) ? startNext - FILL_MIN : Infinity;
      const dx = Math.max(lo, Math.min(Math.max(0, hi), ev.clientX - startX));
      if (!meFill) saveWidth(table, k, share(startW + dx, tableW));
      if (nextK) saveWidth(table, nextK, share(startNext - dx, tableW));
    };
    const up = () => {
      setDragging(false);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };
  const sortable = !!onSort;
  return (
    <th
      data-col={k}
      data-fill={fill ? "1" : undefined}
      className={["group/th relative py-2 font-semibold", align === "right" ? "text-right" : "", className].join(" ")}
      style={width ? { ...style, width: `${(width * 100).toFixed(2)}%` } : style}
      aria-sort={on ? (sort!.dir === "asc" ? "ascending" : "descending") : sortable ? "none" : undefined}
    >
      {sortable ? (
        <button
          type="button"
          onClick={() => onSort!(k, first)}
          className={[
            "group inline-flex max-w-full items-center gap-1 rounded-[6px] hover:text-ink",
            align === "right" ? "flex-row-reverse" : "",
            on ? "text-ink" : "",
          ].join(" ")}
        >
          <span className="truncate">{label}</span>
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            aria-hidden="true"
            className={[
              "shrink-0 transition-[transform,opacity] duration-150",
              on ? "opacity-100" : "opacity-0 group-hover:opacity-50",
              on && sort!.dir === "asc" ? "rotate-180" : "",
            ].join(" ")}
          >
            <path d="M2.5 4l2.5 2.5L7.5 4" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      ) : (
        <span className="truncate">{label}</span>
      )}
      {resize ? (
        <span
          role="separator"
          aria-orientation="vertical"
          aria-label={`Resize ${label}`}
          aria-valuenow={width ? Math.round(width * 100) : undefined}
          aria-valuetext={width ? `${Math.round(width * 100)}% of the table` : "Default width"}
          tabIndex={0}
          title="Drag to resize. Double-click to reset."
          onPointerDown={startDrag}
          onClick={(e) => e.stopPropagation()}
          onDoubleClick={(e) => {
            // The grip moves a pair (this column and the next), so it resets both.
            saveWidth(table, k, null);
            const next = e.currentTarget.parentElement?.nextElementSibling as HTMLElement | null;
            if (next?.dataset.col && next.dataset.fill !== "1") saveWidth(table, next.dataset.col, null);
          }}
          onKeyDown={(e) => {
            if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
            e.preventDefault();
            const th = e.currentTarget.parentElement as HTMLElement;
            const tableW = th.closest("table")!.getBoundingClientRect().width;
            saveWidth(table, k, share(th.getBoundingClientRect().width + (e.key === "ArrowRight" ? 16 : -16), tableW));
          }}
          className="col-grip absolute -right-2 top-0 bottom-0 z-10 flex w-4 cursor-col-resize touch-none select-none justify-center outline-none [&:focus-visible>i]:w-0.5 [&:focus-visible>i]:bg-accent"
        >
          <i
            className={[
              "w-px rounded-full transition-[background-color,width] duration-100",
              dragging ? "w-0.5 bg-accent" : "bg-transparent group-hover/th:bg-rule",
            ].join(" ")}
          />
        </span>
      ) : null}
    </th>
  );
}
