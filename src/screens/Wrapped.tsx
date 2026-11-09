import { PageHeader } from "../components/PageHeader";
import { useEffect, useRef, useState } from "react";
import { ipc } from "../ipc/client";
import { useWrapped } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import { FChip } from "../components/FChip";
import { CARD_H, CARD_W, cardPng, cells, drawCard, loadCardFonts, shares } from "../lib/wrappedCard";
import { formatCount } from "../lib/format";
import { t } from "../strings";

const primary = "h-10 rounded-full bg-ink px-5 text-[14px] font-semibold tracking-[-0.02em] text-bg active:scale-[0.97] disabled:opacity-50";

export function Wrapped() {
  const [year, setYear] = useState<number | null>(null);
  const q = useWrapped(year);
  const w = q.data;
  const canvas = useRef<HTMLCanvasElement>(null);
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState<"idle" | "saving" | "saved">("idle");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    void loadCardFonts().finally(() => live && setReady(true));
    return () => {
      live = false;
    };
  }, []);
  useEffect(() => {
    if (ready && w && canvas.current) drawCard(canvas.current, w);
    setStatus("idle");
  }, [ready, w]);

  const save = async () => {
    if (!canvas.current || !w) return;
    setError(null);
    setStatus("saving");
    try {
      const saved = await ipc.fileSave(await cardPng(canvas.current), `SkillDesk Wrapped ${w.year}.png`);
      setStatus(saved ? "saved" : "idle");
    } catch (e) {
      setError(getErrorMessage(e, t.loadFailed));
      setStatus("idle");
    }
  };

  // Screen readers get the card as text.
  const summary = w
    ? [t.wrappedHead(w.year), `${formatCount(w.prompts)} prompts`, ...cells(w).map(([l, v]) => `${l}: ${v}`), ...shares(w).map((s) => `${s.name} ${formatCount(s.prompts)}`)].join(". ")
    : "";

  return (
    <div className="h-full overflow-auto page">
      <div className="flex flex-col gap-5">
        <div className="flex items-end justify-between gap-4">
          <PageHeader title={t.wrappedTitle} />
          <button type="button" className={primary} disabled={!w || !w.prompts || !ready || status === "saving"} onClick={() => void save()}>
            {status === "saving" ? t.wrappedSaving : t.wrappedSave}
          </button>
        </div>
        <p className="m-0 text-[15px] text-ink-2">{t.wrappedLead}</p>
        {w && w.years.length > 1 ? (
          <div className="flex flex-wrap gap-2">
            {w.years.map((y) => (
              <FChip key={y} label={String(y)} active={w.year === y} onClick={() => setYear(y)} />
            ))}
          </div>
        ) : null}
        {q.error || error ? (
          <p role="alert" className="m-0 text-danger">
            {error ?? getErrorMessage(q.error, t.loadFailed)}
          </p>
        ) : null}
        {w && !w.prompts ? <p className="m-0 text-[14px] text-ink-2">{t.wrappedEmpty}</p> : null}
        <div className={["flex flex-wrap items-start gap-8", w && !w.prompts ? "hidden" : ""].join(" ")}>
          <canvas
            ref={canvas}
            width={CARD_W}
            height={CARD_H}
            role="img"
            aria-label={summary}
            className="h-auto w-[min(480px,100%)] rounded-[22px] shadow-[0_0_0_1px_rgba(17,17,20,.06),0_10px_24px_-12px_rgba(30,20,10,.18)]"
          />
          <div className="flex max-w-[360px] flex-col gap-3 text-[13px] text-ink-2">
            <p className="m-0">{t.wrappedNote}</p>
            <p className="m-0 text-ink-3">{t.wrappedSkillNote}</p>
            {status === "saved" ? (
              <p role="status" className="m-0 font-semibold text-ok">
                {t.wrappedSaved}
              </p>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
