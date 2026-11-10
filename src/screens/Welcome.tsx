import { useEffect, useRef, useState } from "react";
import { AgentLogo } from "../components/AgentLogo";
import { BrandLockup } from "../components/BrandMark";
import type { ItemKind } from "../ipc/bindings/ItemKind";
import type { Overview } from "../ipc/bindings/Overview";
import { useScanStages } from "../ipc/queries";
import hills from "../assets/welcome-hills.webp";
import { introExit, welcomeExit } from "../lib/motion";
import { kindLabel, t } from "../strings";

export type WelcomePhase = "intro" | "scanning" | "done" | "leaving";

/**
 * First run only, on the sky and hills scene (DESIGN.md §5, §6).
 * - intro: one button starts the first scan.
 * - scanning: the intro lifts away and the discovery scene rises in. The
 *   agents found, a live tally per kind and a stage bar, until the Dashboard
 *   has its data.
 * - done: "Found on this Mac", the bar completes; a short hold.
 * - leaving: the scene lifts away over the app shell; `onGone` when clear.
 */
export function Welcome({ data, phase, onScan, onGone }: { data: Overview; phase: WelcomePhase; onScan: () => void; onGone?: () => void }) {
  const found = data.agents.filter((a) => a.tier !== "shared");
  const named = found.slice(0, 3);
  const known = data.known_agents;
  // The photo fades in once decoded, so the sky never shows a half-drawn image.
  const [ready, setReady] = useState(false);
  const root = useRef<HTMLElement>(null);
  const intro = phase === "intro";
  useEffect(() => {
    if (phase === "scanning" && root.current) introExit(root.current);
    if (phase !== "leaving" || !root.current) return;
    let live = true;
    void welcomeExit(root.current).then(() => live && onGone?.());
    return () => {
      live = false;
    };
  }, [phase, onGone]);
  return (
    <main
      ref={root}
      aria-hidden={phase === "leaving" || undefined}
      className={["welcome-scene fixed inset-0 isolate z-50 flex flex-col overflow-auto", phase === "leaving" ? "pointer-events-none" : ""].join(" ")}
      data-tauri-drag-region
    >
      <img
        src={hills}
        alt=""
        aria-hidden="true"
        data-hills
        draggable={false}
        onLoad={() => setReady(true)}
        className={[
          "pointer-events-none absolute inset-x-0 bottom-0 -z-10 h-[60%] w-full select-none object-cover object-center transition-opacity duration-700 motion-reduce:transition-none",
          ready ? "opacity-100" : "opacity-0",
        ].join(" ")}
      />
      <div className="h-12 shrink-0" data-tauri-drag-region />
      <div className="relative mx-auto grid w-full max-w-[760px] flex-1 px-6 pb-16 pt-[6vh] text-center">
        <div data-intro inert={!intro} className="col-start-1 row-start-1 flex flex-col items-center gap-5">
          <span data-lift>
            <BrandLockup size={44} />
          </span>
          <h1 data-lift className="m-0 max-w-[18ch] text-balance text-[60px] font-bold leading-[1.02] tracking-[-0.05em] text-ink max-[720px]:text-[40px]">
            {t.welcomeTitle}
          </h1>
          <p data-lift className="m-0 max-w-[52ch] text-[16px] text-ink-2">
            {t.welcomeLead}
          </p>
          <button data-lift type="button" onClick={onScan} disabled={!intro} className="mt-2 h-12 rounded-full bg-ink px-7 text-[15px] font-semibold text-bg transition-transform duration-100 active:scale-[0.97]">
            {t.scanThisMac}
          </button>
          {named.length ? (
            <div data-lift className="welcome-glass mt-6 inline-flex items-center gap-3.5 rounded-full py-2 pl-2 pr-[18px] text-left text-[14px] text-ink-2 max-[640px]:flex-col max-[640px]:rounded-[20px] max-[640px]:px-4 max-[640px]:py-3 max-[640px]:text-center">
              <span className="flex" aria-hidden="true">
                {named.map((a, i) => (
                  <span key={a.key} className={["grid h-[34px] w-[34px] place-items-center rounded-full bg-bg ring-2 ring-bg", i ? "-ml-2" : ""].join(" ")}>
                    <AgentLogo agent={a} size={16} />
                  </span>
                ))}
              </span>
              <span>
                {t.worksWithLead}{" "}
                {named.map((a, i) => (
                  <span key={a.key}>
                    {i ? ", " : ""}
                    <b className="font-semibold text-ink">{a.display_name}</b>
                  </span>
                ))}{" "}
                and <b className="font-semibold text-ink">{t.worksWith(Math.max(0, known - named.length))}</b> coding agents
              </span>
            </div>
          ) : null}
        </div>
        {intro ? null : <Discovery data={data} done={phase === "done" || phase === "leaving"} />}
      </div>
    </main>
  );
}

const SHOWN_AGENTS = 7;
const TALLY: ItemKind[] = ["skill", "plugin", "hook", "mcp_server", "subagent", "memory"];
// The stages the Dashboard waits for; secrets and storage finish behind it.
const STEPS = [
  { key: "config", label: t.stageConfig },
  { key: "transcripts", label: t.stageSessions },
] as const;

/** The discovery scene: what SkillDesk finds, as it finds it. */
function Discovery({ data, done }: { data: Overview; done: boolean }) {
  const stages = useScanStages();
  const agents = data.agents.filter((a) => a.tier !== "shared");
  const more = agents.length - SHOWN_AGENTS;
  const doneSteps = STEPS.filter((s) => stages.includes(s.key)).length;
  const step = STEPS.find((s) => !stages.includes(s.key));
  // Half a step for the one running, so the bar moves as soon as it starts.
  const progress = done ? 1 : Math.min(0.92, (doneSteps + 0.5) / STEPS.length);
  const count = (k: ItemKind) => data.kinds.find((c) => c.kind === k)?.count ?? 0;
  const status = done ? t.foundOnThisMac : t.readingThisMac;
  return (
    <section data-scan aria-busy={!done} className="col-start-1 row-start-1 flex flex-col items-center gap-7 self-center">
      <div data-lift className="relative flex items-center justify-center">
        <span className="scan-halo" aria-hidden="true" />
        <ul className="relative m-0 flex list-none flex-wrap justify-center gap-2.5 p-0" aria-label={t.agentsT}>
          {agents.slice(0, SHOWN_AGENTS).map((a, i) => (
            <li key={a.key} className="scene-in welcome-glass grid h-14 w-14 place-items-center rounded-full" style={{ animationDelay: `${160 + i * 70}ms` }} title={a.display_name}>
              <AgentLogo agent={a} size={24} />
            </li>
          ))}
          {more > 0 ? (
            <li className="scene-in welcome-glass grid h-14 w-14 place-items-center rounded-full text-[14px] font-semibold text-ink-2 tnum" style={{ animationDelay: `${160 + SHOWN_AGENTS * 70}ms` }}>
              +{more}
            </li>
          ) : null}
        </ul>
      </div>

      <div data-lift className="scene-in flex flex-col items-center gap-3" style={{ animationDelay: "120ms" }} role="status" aria-live="polite">
        <h2 key={status} className="swap-in m-0 text-[44px] font-bold leading-[1.05] tracking-[-0.05em] text-ink max-[720px]:text-[32px]">
          {status}
        </h2>
        <p key={step?.key ?? "done"} className="swap-in m-0 inline-flex items-center gap-2 text-[15px] text-ink-2">
          {done || !step ? (
            <svg width="14" height="14" viewBox="0 0 12 12" aria-hidden="true" className="stage-check text-ok">
              <path d="M2.5 6.2 5 8.5l4.5-5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          ) : (
            <i className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" aria-hidden="true" />
          )}
          {done || !step ? t.welcomeReady : step.label}
        </p>
      </div>

      <div data-lift className="scene-in w-[280px] max-w-full" style={{ animationDelay: "200ms" }} aria-hidden="true">
        <div className="scene-track relative h-1 overflow-hidden rounded-full">
          <span className="scene-fill absolute inset-0 rounded-full bg-accent" style={{ transform: `scaleX(${progress})` }} />
          {done ? null : <span className="scene-sheen absolute inset-0" />}
        </div>
      </div>

      <dl data-lift className="scene-in welcome-glass m-0 grid grid-cols-6 rounded-[20px] px-2 py-3 max-[720px]:grid-cols-3" style={{ animationDelay: "280ms" }}>
        {TALLY.map((k) => (
          <div key={k} className="flex min-w-[96px] flex-col items-center gap-0.5 px-3 py-1">
            <dd className="m-0 text-[24px] font-semibold leading-tight tracking-[-0.02em] text-ink tnum">
              <Tally value={count(k)} />
            </dd>
            <dt className="text-[12.5px] text-ink-2">{kindLabel[k]}</dt>
          </div>
        ))}
      </dl>
    </section>
  );
}

const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** A number that runs from its last value to the new one (500ms, ease-out) each time it changes. */
function Tally({ value }: { value: number }) {
  const [shown, setShown] = useState(0);
  const from = useRef(0);
  useEffect(() => {
    if (reduced()) {
      from.current = value;
      setShown(value);
      return;
    }
    const start = performance.now();
    const a = from.current;
    let raf = 0;
    const tick = (now: number) => {
      const p = Math.min(1, (now - start) / 500);
      const v = Math.round(a + (value - a) * (1 - Math.pow(1 - p, 3)));
      from.current = v;
      setShown(v);
      if (p < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [value]);
  return <>{shown.toLocaleString("en-US")}</>;
}
