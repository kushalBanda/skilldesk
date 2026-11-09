// Wrapped card (DESIGN.md §7.13): 1080×1350, always dark, drawn on a canvas
// so the preview and the saved PNG are the same pixels.
import type { WrappedData } from "../ipc/bindings/WrappedData";
import { agentColorDark, OTHER_DARK } from "./series";
import { formatCount } from "./format";
import { t } from "../strings";

export const CARD_W = 1080;
export const CARD_H = 1350;

const FONT = '"Inter Variable", Inter, system-ui, sans-serif';
const INK = "#FFFFFF";
const MUTED = "#7B828E";
const SOFT = "#A3A9B3";
const PAD_X = 84;

/** Waits for the weights the card uses; canvas text does not wait by itself. */
export async function loadCardFonts() {
  await Promise.all([500, 600, 700].map((w) => document.fonts.load(`${w} 48px "Inter Variable"`)));
}

const font = (weight: number, px: number) => `${weight} ${px}px ${FONT}`;

/** Text with tracking in em; uses canvas letterSpacing where WebKit has it. */
function text(ctx: CanvasRenderingContext2D, s: string, x: number, y: number, px: number, trackEm: number) {
  const c = ctx as CanvasRenderingContext2D & { letterSpacing?: string };
  if ("letterSpacing" in c) {
    c.letterSpacing = `${trackEm * px}px`;
    ctx.fillText(s, x, y);
    c.letterSpacing = "0px";
    return;
  }
  const chars = [...s];
  chars.forEach((ch, i) => ctx.fillText(ch, x + ctx.measureText(chars.slice(0, i).join("")).width + i * trackEm * px, y));
}

function width(ctx: CanvasRenderingContext2D, s: string, px: number, trackEm: number) {
  return ctx.measureText(s).width + [...s].length * trackEm * px;
}

/** Largest size from `px` down that fits `max`; adds "…" when even the floor does not fit. */
function fit(ctx: CanvasRenderingContext2D, s: string, weight: number, px: number, floor: number, max: number, trackEm: number): [string, number] {
  for (let size = px; size >= floor; size -= 2) {
    ctx.font = font(weight, size);
    if (width(ctx, s, size, trackEm) <= max) return [s, size];
  }
  ctx.font = font(weight, floor);
  let cut = s;
  while (cut.length > 1 && width(ctx, `${cut}…`, floor, trackEm) > max) cut = cut.slice(0, -1);
  return [`${cut}…`, floor];
}

const hour12 = (h: number) => `${h % 12 === 0 ? 12 : h % 12} ${h < 12 ? "AM" : "PM"}`;

function longDate(day: string) {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y!, m! - 1, d!).toLocaleDateString("en-GB", { day: "numeric", month: "long" });
}

type Cell = [label: string, value: string];

/** Six cells, in this order of preference; missing numbers give way to the next. */
export function cells(w: WrappedData): Cell[] {
  const all: (Cell | null)[] = [
    w.peak_hour !== null ? [t.wrappedPeakHour, hour12(w.peak_hour)] : null,
    w.top_weekday !== null ? [t.wrappedTopDay, t.weekdays[w.top_weekday] ?? ""] : null,
    w.longest_streak > 0 ? [t.wrappedStreak, t.daysN(w.longest_streak)] : null,
    w.top_skill ? [t.wrappedTopSkill, w.top_skill.name] : null,
    w.top_command ? [t.wrappedTyped(w.top_command.name), `${formatCount(w.top_command.count)}×`] : null,
    w.busiest_day ? [t.wrappedBusiest, formatCount(w.busiest_day.prompts)] : null,
    w.active_days > 0 ? [t.wrappedActiveDays, formatCount(w.active_days)] : null,
    w.agents.length ? [t.wrappedAgents, String(w.agents.length)] : null,
  ];
  return all.filter((c): c is Cell => c !== null).slice(0, 6);
}

/** Agents under 1 % fold into one "Other" share. */
export function shares(w: WrappedData) {
  const total = Math.max(w.prompts, 1);
  const big = w.agents.filter((a) => a.prompts / total >= 0.01);
  const rest = w.agents.filter((a) => a.prompts / total < 0.01).reduce((n, a) => n + a.prompts, 0);
  const out = big.map((a) => ({ name: a.name, prompts: a.prompts, color: agentColorDark(a.agent) }));
  if (rest > 0) out.push({ name: t.wrappedOther, prompts: rest, color: OTHER_DARK });
  return out;
}

const pct = (n: number, total: number) => {
  const p = (n / Math.max(total, 1)) * 100;
  return p < 1 ? "<1%" : `${Math.round(p)}%`;
};

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
  ctx.fill();
}

function mark(ctx: CanvasRenderingContext2D, x: number, y: number) {
  const g = ctx.createLinearGradient(0, y, 0, y + 40);
  g.addColorStop(0, "#2B2B30");
  g.addColorStop(1, "#0B0B0C");
  ctx.fillStyle = g;
  roundRect(ctx, x, y, 40, 40, 11);
  ctx.strokeStyle = "rgba(255,255,255,.12)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.roundRect(x + 0.5, y + 0.5, 39, 39, 10.5);
  ctx.stroke();
  // Glyph: 24-unit grid at 24px, centred (DESIGN.md §2 Brand).
  const gx = x + 8;
  const gy = y + 8;
  ctx.fillStyle = "#FFFFFF";
  roundRect(ctx, gx + 4, gy + 4, 12.5, 7.5, 3.75);
  ctx.fillStyle = "#0099FF";
  roundRect(ctx, gx + 7.5, gy + 12.5, 12.5, 7.5, 3.75);
}

/** The companion as a plain penguin; its name stays private. */
function companion(ctx: CanvasRenderingContext2D, x: number, y: number) {
  ctx.save();
  ctx.translate(x, y);
  ctx.scale(1.25, 1.25);
  const p = (d: string, fill: string, stroke?: string) => {
    const path = new Path2D(d);
    ctx.fillStyle = fill;
    ctx.fill(path);
    if (stroke) {
      ctx.strokeStyle = stroke;
      ctx.lineWidth = 1;
      ctx.stroke(path);
    }
  };
  p("M60 10c-24 0-38 22-38 52 0 32 14 58 38 58s38-26 38-58c0-30-14-52-38-52z", "#2E2E33", "rgba(255,255,255,.18)");
  p("M60 40c-15 0-23 16-23 36 0 22 9 38 23 38s23-16 23-38c0-20-8-36-23-36z", "#FFFDF7");
  const dot = (cx: number, cy: number, r: number, fill: string) => {
    ctx.fillStyle = fill;
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.fill();
  };
  dot(49, 40, 4.5, "#FFFFFF");
  dot(71, 40, 4.5, "#FFFFFF");
  dot(50, 41, 2.4, "#000000");
  dot(72, 41, 2.4, "#000000");
  p("M54 50l6 7 6-7z", "#F29D49");
  p("M22 66c-8 8-10 22-6 30 6-4 9-14 10-24z", "#2E2E33");
  p("M98 66c8 8 10 22 6 30-6-4-9-14-10-24z", "#2E2E33");
  p("M44 118c-6 0-10 3-10 6h18c0-3-3-6-8-6zM76 118c6 0 10 3 10 6H68c0-3 3-6 8-6z", "#F29D49");
  ctx.restore();
}

export function drawCard(canvas: HTMLCanvasElement, w: WrappedData) {
  canvas.width = CARD_W;
  canvas.height = CARD_H;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = "#0D0D0D";
  ctx.fillRect(0, 0, CARD_W, CARD_H);
  const inner = CARD_W - PAD_X * 2;

  ctx.fillStyle = INK;
  ctx.font = font(700, 76);
  text(ctx, t.wrappedHead(w.year), PAD_X, 80 + 62, 76, -0.05);

  // Hero number.
  const [big, bigPx] = fit(ctx, formatCount(w.prompts), 700, 220, 120, inner, -0.06);
  ctx.font = font(700, bigPx);
  text(ctx, big, PAD_X - 6, 214 + bigPx * 0.72, bigPx, -0.06);
  const current = w.year === new Date().getFullYear();
  ctx.fillStyle = SOFT;
  ctx.font = font(500, 28);
  text(ctx, current && w.first_day ? t.wrappedSince(longDate(w.first_day)) : t.wrappedIn(w.year), PAD_X, 470, 28, -0.01);

  // Six cells, 3 × 2, under a 2px rule.
  const top = 524;
  ctx.fillStyle = INK;
  ctx.fillRect(PAD_X, top, inner, 2);
  const colW = inner / 3;
  const rowH = 132;
  cells(w).forEach(([label, value], i) => {
    const x = PAD_X + (i % 3) * colW;
    const y = top + 2 + Math.floor(i / 3) * rowH;
    ctx.fillStyle = MUTED;
    const [l, lPx] = fit(ctx, label, 500, 20, 16, colW - 20, 0);
    ctx.font = font(500, lPx);
    text(ctx, l, x, y + 22 + 18, lPx, 0);
    ctx.fillStyle = INK;
    const [v, vPx] = fit(ctx, value, 700, 48, 28, colW - 20, -0.04);
    ctx.font = font(700, vPx);
    text(ctx, v, x, y + 22 + 30 + 48, vPx, -0.04);
    ctx.fillStyle = "rgba(255,255,255,.10)";
    ctx.fillRect(x, y + rowH - 1, colW, 1);
  });

  // Prompts by agent: one bar with 3px gaps, then a legend.
  const parts = shares(w);
  if (parts.length) {
    const sy = top + 2 + rowH * 2 + 44;
    ctx.fillStyle = INK;
    ctx.font = font(600, 20);
    text(ctx, t.wrappedByAgent, PAD_X, sy + 16, 20, -0.02);
    const by = sy + 36;
    const gaps = (parts.length - 1) * 3;
    ctx.save();
    ctx.beginPath();
    ctx.roundRect(PAD_X, by, inner, 28, 8);
    ctx.clip();
    let x = PAD_X;
    for (const s of parts) {
      const segW = Math.max(4, ((inner - gaps) * s.prompts) / Math.max(w.prompts, 1));
      ctx.fillStyle = s.color;
      ctx.fillRect(x, by, segW, 28);
      x += segW + 3;
    }
    ctx.restore();
    let lx = PAD_X;
    const ly = by + 28 + 34;
    ctx.font = font(500, 18);
    for (const s of parts) {
      const label = `${s.name} ${pct(s.prompts, w.prompts)}`;
      const lw = 14 + 10 + width(ctx, label, 18, 0);
      if (lx + lw > PAD_X + inner) break;
      ctx.fillStyle = s.color;
      roundRect(ctx, lx, ly - 13, 14, 14, 4);
      ctx.fillStyle = SOFT;
      text(ctx, label, lx + 24, ly, 18, 0);
      lx += lw + 26;
    }
  }

  // Footer: mark + name, the privacy line, the companion.
  const fy = CARD_H - 64;
  mark(ctx, PAD_X, fy - 40 - 32);
  ctx.fillStyle = INK;
  ctx.font = font(700, 26);
  text(ctx, "SkillDesk", PAD_X + 52, fy - 32 - 11, 26, -0.04);
  ctx.fillStyle = MUTED;
  ctx.font = font(400, 17);
  text(ctx, t.wrappedFoot, PAD_X, fy - 4, 17, 0);
  if (w.companion) companion(ctx, CARD_W - PAD_X - 150, fy - 166);
}

/** PNG bytes of the drawn card. */
export function cardPng(canvas: HTMLCanvasElement): Promise<Uint8Array> {
  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? b.arrayBuffer().then((a) => resolve(new Uint8Array(a)), reject) : reject(new Error("The card could not be drawn."))), "image/png"),
  );
}
