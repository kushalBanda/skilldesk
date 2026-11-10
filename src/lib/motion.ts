// Motion for changes (DESIGN.md "Motion"): rows that leave, rows that close
// the gap, the trip to History, rows that come back. Web Animations only, so
// every animation starts from what is on screen and can be cancelled midway.
// Reduce motion: opacity only, no travel.
import type { ActionRequest } from "../ipc/bindings/ActionRequest";

/** Close to a critically damped spring with a 0.3 s response (no overshoot). */
export const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";
const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const rowsIn = (root: ParentNode = document) => [...root.querySelectorAll<HTMLElement>("main [data-row-id]")];
const rowFor = (id: string) => rowsIn().filter((el) => el.dataset.rowId === id);

/** What a change removes from the lists, and how it leaves. */
export function departure(req: ActionRequest): { ids: string[]; how: "trash" | "merge" } | null {
  switch (req.type) {
    case "memory_promote":
      return { ids: req.ids, how: "merge" };
    case "memory_forget":
    case "links_remove_dangling":
    case "marketplace_remove":
    case "cache_clean":
      return { ids: req.ids, how: "trash" };
    case "link_remove":
      return { ids: [req.path], how: "trash" };
    case "hook_remove":
      return { ids: req.ids, how: "trash" };
    case "plugin_forget":
      return { ids: [req.id], how: "trash" };
    case "storage_trash":
      return { ids: req.kinds, how: "trash" };
    default:
      return null;
  }
}

/** Positions of the rows on screen now (before a change lands). */
export function snapshot(): Map<string, { x: number; y: number }> {
  return new Map(
    rowsIn().map((el) => {
      const r = el.getBoundingClientRect();
      return [el.dataset.rowId!, { x: r.left, y: r.top }];
    }),
  );
}

export type Leaving = { done: Promise<void>; cancel: () => void; from: DOMRect | null };

/**
 * Starts the rows on their way out while the write runs. Trash: a red tint,
 * then a slide toward the sidebar and fade. Merge: every row slides into the
 * first one and fades, so they read as becoming one.
 */
export function leave(ids: string[], how: "trash" | "merge"): Leaving {
  const els = ids.flatMap(rowFor);
  const from = els[0]?.getBoundingClientRect() ?? null;
  if (!els.length) return { done: Promise.resolve(), cancel: () => {}, from };
  const rm = reduced();
  const target = els[0]!.getBoundingClientRect().top;
  const anims = els.map((el, i) => {
    el.style.pointerEvents = "none";
    if (rm) return el.animate([{ opacity: 1 }, { opacity: 0.25 }], { duration: 160, fill: "forwards" });
    if (how === "merge") {
      const dy = target - el.getBoundingClientRect().top;
      return el.animate(
        [
          { transform: "none", opacity: 1 },
          { transform: `translateY(${dy}px) scale(0.98)`, opacity: i === 0 ? 0.6 : 0, offset: 0.7 },
          { transform: `translateY(${dy}px) scale(0.96)`, opacity: 0 },
        ],
        { duration: 420, easing: EASE, fill: "forwards" },
      );
    }
    return el.animate(
      [
        { transform: "none", opacity: 1, backgroundColor: "transparent" },
        { transform: "none", opacity: 1, backgroundColor: "var(--danger-soft)", offset: 0.25 },
        { transform: "translateX(-24px) scale(0.985)", opacity: 0, backgroundColor: "var(--danger-soft)" },
      ],
      { duration: 320, delay: i * 30, easing: EASE, fill: "forwards" },
    );
  });
  return {
    done: Promise.all(anims.map((a) => a.finished.catch(() => undefined))).then(() => undefined),
    from,
    cancel: () =>
      anims.forEach((a, i) => {
        // Back from where it is now, not from the start.
        a.reverse();
        a.finished.then(() => {
          a.cancel();
          els[i]!.style.pointerEvents = "";
        }, () => (els[i]!.style.pointerEvents = ""));
      }),
  };
}

/**
 * After the lists re-render: rows that moved glide from their old place to the
 * new one (the gap closes); rows that are new fade in with a soft highlight.
 */
export function settle(before: Map<string, { x: number; y: number }>) {
  const rm = reduced();
  const rows = rowsIn();
  // Another screen now (the change navigated): nothing to glide or highlight.
  if (!rows.some((el) => before.has(el.dataset.rowId!))) return;
  for (const el of rows) {
    const id = el.dataset.rowId!;
    const was = before.get(id);
    el.style.pointerEvents = "";
    // A row that was leaving but is still listed comes back to full view.
    el.getAnimations().forEach((a) => a.cancel());
    if (was === undefined) {
      el.animate(
        rm
          ? [{ opacity: 0 }, { opacity: 1 }]
          : [
              { opacity: 0, transform: "translateY(-6px)", backgroundColor: "var(--accent-soft)" },
              { opacity: 1, transform: "none", backgroundColor: "var(--accent-soft)", offset: 0.4 },
              { opacity: 1, transform: "none", backgroundColor: "transparent" },
            ],
        { duration: rm ? 160 : 900, easing: EASE },
      );
      continue;
    }
    const r = el.getBoundingClientRect();
    const dx = was.x - r.left;
    const dy = was.y - r.top;
    if (rm || (Math.abs(dx) < 1 && Math.abs(dy) < 1)) continue;
    el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: 340, easing: EASE });
  }
}

/** A small pill flies from the row to History in the sidebar, which bumps: the change lives there now (and its Undo). */
export function toHistory(from: DOMRect | null, label: string) {
  const target = document.querySelector<HTMLElement>('nav a[href="#/history"]');
  if (!target) return;
  const bump = () =>
    target.animate(
      [{ transform: "none" }, { transform: "scale(1.06)", backgroundColor: "var(--accent-soft)", offset: 0.35 }, { transform: "none" }],
      { duration: 420, easing: EASE },
    );
  if (!from || reduced()) return void bump();
  const to = target.getBoundingClientRect();
  const pill = document.createElement("div");
  pill.textContent = label;
  pill.setAttribute("aria-hidden", "true");
  pill.className = "fly-pill";
  document.body.appendChild(pill);
  const x0 = from.left + 16;
  const y0 = from.top + from.height / 2 - 14;
  const x1 = to.left + 12;
  const y1 = to.top + to.height / 2 - 14;
  // A shallow arc: up a little first, then down into History.
  const mx = (x0 + x1) / 2;
  const my = Math.min(y0, y1) - 40;
  const a = pill.animate(
    [
      { transform: `translate(${x0}px, ${y0}px) scale(1)`, opacity: 0 },
      { transform: `translate(${x0}px, ${y0}px) scale(1)`, opacity: 1, offset: 0.12 },
      { transform: `translate(${mx}px, ${my}px) scale(0.8)`, opacity: 1, offset: 0.55 },
      { transform: `translate(${x1}px, ${y1}px) scale(0.35)`, opacity: 0.2 },
    ],
    { duration: 620, easing: "cubic-bezier(0.45, 0, 0.2, 1)" },
  );
  a.finished.then(
    () => {
      pill.remove();
      bump();
    },
    () => pill.remove(),
  );
}

/** A row reached by a link (⌘K, Fix list) pulses once so the eye finds it. */
export function pulse(el: Element | null) {
  if (!el) return;
  el.animate(
    reduced()
      ? [{ opacity: 0.6 }, { opacity: 1 }]
      : [{ boxShadow: "inset 0 0 0 2px var(--accent)" }, { boxShadow: "inset 0 0 0 2px transparent" }],
    { duration: 900, easing: EASE },
  );
}

/** A new screen fades in and rises a few pixels; nothing slides sideways. */
let screenAt = 0;
/** True right after a screen change: a sheet open now came from a link, not a click on its row. */
export const justArrived = () => performance.now() - screenAt < 800;

export function enterScreen(el: Element | null) {
  screenAt = performance.now();
  if (!el) return;
  el.animate(
    reduced() ? [{ opacity: 0 }, { opacity: 1 }] : [{ opacity: 0, transform: "translateY(6px)" }, { opacity: 1, transform: "none" }],
    { duration: 220, easing: EASE },
  );
}

// First run: Welcome hands over to the Dashboard (DESIGN.md §5).
// Act 1, discovery: the intro lifts away and the discovery scene rises in on
// the same sky while the scan runs (Welcome.tsx).
// Act 2, lift-off: when the Dashboard has its data, the scene rises away, the
// hills sink, the sky clears, and the app shell settles in underneath.
// Act 3, reveal: the Dashboard sections rise in order and the activity grid
// wipes in, oldest week to today.

/** Lift-off length: the Welcome overlay is fully clear after this (ms). */
export const WELCOME_EXIT_MS = 660;

// Smooth on WebKit: only transform (translate, never scale) and opacity move,
// so the compositor runs every frame and text is never re-rasterised. No
// animated blur, and the glass bar drops its backdrop blur while it leaves.
const promote = (el: HTMLElement, props: string) => {
  el.style.willChange = props;
  return el;
};

/** Rise and fade out, top to bottom; stays hidden (fill forwards). */
function liftAway(els: HTMLElement[], duration: number, gap: number) {
  els.forEach((el, i) =>
    promote(el, "transform, opacity").animate(
      [
        { opacity: 1, transform: "translateY(0)" },
        { opacity: 0, transform: "translateY(-24px)" },
      ],
      { duration, delay: i * gap, easing: EASE, fill: "forwards" },
    ),
  );
}

/** Scan pressed: the intro lifts away; the discovery scene rises in on its own (CSS `.scene-in`). */
export function introExit(root: HTMLElement) {
  const els = [...root.querySelectorAll<HTMLElement>("[data-intro] [data-lift]")];
  if (reduced()) els.forEach((el) => el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 200, easing: "linear", fill: "forwards" }));
  else liftAway(els, 360, 30);
}

/** Welcome leaves. Resolves when the overlay is fully gone. */
export function welcomeExit(root: HTMLElement): Promise<void> {
  const fill = { fill: "forwards" as const };
  root.dataset.leaving = "";
  if (reduced()) {
    return root.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 240, easing: "linear", ...fill }).finished.then(() => {});
  }
  const hills = root.querySelector<HTMLElement>("[data-hills]");
  liftAway([...root.querySelectorAll<HTMLElement>("[data-scan] [data-lift]")], 440, 40);
  if (hills)
    promote(hills, "transform, opacity").animate(
      [
        { opacity: 1, transform: "translateY(0)" },
        { opacity: 0, transform: "translateY(10%)" },
      ],
      { duration: 600, delay: 60, easing: EASE, ...fill },
    );
  return promote(root, "opacity")
    .animate([{ opacity: 1 }, { opacity: 0 }], { duration: 360, delay: WELCOME_EXIT_MS - 360, easing: EASE, ...fill })
    .finished.then(() => {});
}

/** The app shell arrives under the leaving Welcome: faded and 8 px low, to rest. */
export function shellEnter(el: HTMLElement | null) {
  if (!el) return;
  if (reduced()) {
    el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: "linear" });
    return;
  }
  promote(el, "transform, opacity");
  el.animate(
    [
      { opacity: 0, transform: "translateY(8px)" },
      { opacity: 1, transform: "translateY(0)" },
    ],
    { duration: 520, delay: 240, easing: EASE, fill: "backwards" },
  ).finished.then(
    () => (el.style.willChange = ""),
    () => (el.style.willChange = ""),
  );
}

/** Sections rise in, one after another (60 ms apart), when the first prompts arrive. */
export function revealSections(root: HTMLElement | null) {
  if (!root) return;
  const rm = reduced();
  // The page header stays put; everything under it rises.
  [...root.children].slice(1).forEach((el, i) => {
    el.animate(
      rm ? [{ opacity: 0 }, { opacity: 1 }] : [{ opacity: 0, transform: "translateY(12px)" }, { opacity: 1, transform: "none" }],
      { duration: rm ? 200 : 520, delay: rm ? 0 : i * 60, easing: EASE, fill: "backwards" },
    );
  });
}

/**
 * Activity grid sweeps in left to right, oldest week first, about 0.8 s. One
 * clip on the whole grid, not one animation per cell, so it stays smooth.
 */
export function sweepGrid(grid: HTMLElement | null) {
  if (!grid || reduced()) return;
  grid.animate(
    [
      { clipPath: "inset(0 100% 0 0)", opacity: 0.4 },
      { clipPath: "inset(0 0 0 0)", opacity: 1 },
    ],
    { duration: 800, delay: 240, easing: EASE, fill: "backwards" },
  );
}
