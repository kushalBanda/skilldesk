import { AgentLogo } from "../components/AgentLogo";
import { BrandLockup } from "../components/BrandMark";
import type { AgentSummary } from "../ipc/bindings/AgentSummary";
import { t } from "../strings";

/** First run only (no scan yet): one button starts the first scan. */
export function Welcome({ agents, known, onScan }: { agents: AgentSummary[]; known: number; onScan: () => void }) {
  const found = agents.filter((a) => a.tier !== "shared");
  const named = found.slice(0, 3);
  return (
    <main className="relative flex h-full flex-col overflow-auto bg-bg" data-tauri-drag-region>
      <div className="h-12 shrink-0" data-tauri-drag-region />
      <div className="mx-auto flex max-w-[760px] flex-1 flex-col items-center justify-center gap-5 px-6 pb-16 text-center">
        <BrandLockup size={44} />
        <h1 className="m-0 max-w-[18ch] text-balance text-[60px] font-bold leading-[1.02] tracking-[-0.05em] max-[720px]:text-[40px]">{t.welcomeTitle}</h1>
        <p className="m-0 max-w-[52ch] text-[16px] text-ink-2">{t.welcomeLead}</p>
        <button type="button" onClick={onScan} className="mt-2 h-12 rounded-full bg-ink px-7 text-[15px] font-semibold text-bg active:scale-[0.97]">
          {t.scanThisMac}
        </button>
        <p className="m-0 text-[12.5px] text-ink-3">{t.welcomeNote}</p>
        {named.length ? (
          <div className="mt-6 inline-flex items-center gap-3.5 rounded-full bg-panel py-2 pl-2 pr-[18px] text-left text-[14px] text-ink-2 ring-1 ring-rule">
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
    </main>
  );
}
