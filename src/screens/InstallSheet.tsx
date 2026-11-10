// Install skill sheet (DESIGN.md §4 "Install sheet", §7.14): drop zone or
// picker, then what SKILL.md says, then agent chips, then one CTA. The CTA
// copies the skill into each picked agent's folder through the action engine;
// a clash opens Replace preview.
import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { ipc } from "../ipc/client";
import { getErrorMessage } from "../ipc/error";
import { useOverview } from "../ipc/queries";
import { useAction } from "../actions/ActionProvider";
import { DetailSheet } from "../components/DetailSheet";
import { AgentChips, ClashNotes } from "../components/AgentChips";
import { formatBytes, formatCount, tildePath } from "../lib/format";
import { t } from "../strings";

export const DEFAULT_ON = ["claude_code", "cursor"];

export function InstallSheet({ onClose, onInstalled }: { onClose: () => void; onInstalled: (name: string) => void }) {
  const [path, setPath] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const agents = useOverview().data?.agents ?? [];
  const { run } = useAction();

  useEffect(() => {
    let off: (() => void) | undefined;
    let alive = true;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setOver(true);
        else if (e.payload.type === "leave") setOver(false);
        else if (e.payload.type === "drop") {
          setOver(false);
          if (e.payload.paths[0]) setPath(e.payload.paths[0]);
        }
      })
      .then((u) => (alive ? (off = u) : u()));
    return () => {
      alive = false;
      off?.();
    };
  }, []);

  const preview = useQuery({
    queryKey: ["skill-inspect", path],
    queryFn: () => ipc.skillInspect(path!),
    enabled: path !== null,
    retry: false,
    gcTime: 0,
  });
  const p = preview.data;
  const [chosen, setChosen] = useState<string[] | null>(null);
  const picked = useMemo(
    () => chosen ?? (p ? p.targets.filter((x) => DEFAULT_ON.includes(x.key)).map((x) => x.key) : []),
    [chosen, p],
  );
  useEffect(() => setChosen(null), [path]);

  const pick = async (directory: boolean) => {
    const res = await open(directory ? { directory: true } : { filters: [{ name: "Skill", extensions: ["zip", "skill"] }] });
    if (typeof res === "string") setPath(res);
  };
  const n = picked.length;

  return (
    <DetailSheet title={t.installSkill} onClose={onClose} width={480}>
      <h2 className="m-0 text-[24px] font-bold leading-tight tracking-[-0.03em]">{t.installSkill}</h2>
      {path === null || preview.isError ? (
        <div
          className={[
            "mt-5 flex flex-col items-center gap-3 rounded-panel border-[1.5px] border-dashed px-6 py-10 text-center",
            over ? "border-accent bg-accent-soft" : "border-rule bg-panel",
          ].join(" ")}
        >
          <p className="m-0 font-semibold">{t.dropHere}</p>
          <p className="m-0 text-[13px] text-ink-3">{t.dropKinds}</p>
          <div className="mt-2 flex flex-wrap justify-center gap-2">
            <button type="button" onClick={() => void pick(true)} className="h-9 rounded-full bg-bg px-4 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]">
              {t.chooseFolder}
            </button>
            <button type="button" onClick={() => void pick(false)} className="h-9 rounded-full bg-bg px-4 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]">
              {t.chooseArchive}
            </button>
          </div>
          {preview.isError ? (
            <p role="alert" className="m-0 mt-2 text-[13px] text-danger">
              {getErrorMessage(preview.error, t.loadFailed)}
            </p>
          ) : null}
        </div>
      ) : null}
      {p ? (
        <>
          <div className="mt-5 flex items-center gap-3 rounded-row bg-panel px-3 py-2.5">
            <div className="min-w-0 flex-1">
              <div className="selectable truncate font-mono text-[12.5px]" title={p.source}>
                {tildePath(p.source)}
              </div>
              <div className="text-[12.5px] text-ink-3">
                {t.sourceKind[p.kind]} · {formatCount(p.files)} {p.files === 1 ? t.file : t.files} · {formatBytes(p.bytes)}
              </div>
            </div>
            <button type="button" onClick={() => setPath(null)} className="text-[13px] font-semibold text-accent-text hover:underline">
              {t.change}
            </button>
          </div>
          <h3 className="m-0 mt-5 break-words text-[20px] font-bold tracking-[-0.03em]">{p.name}</h3>
          {p.description ? <p className="mt-1 text-ink-2">{p.description}</p> : null}
          <ClashNotes name={p.name} targets={p.targets} picked={picked} agents={agents} />
          <AgentChips targets={p.targets} picked={picked} onChange={setChosen} agents={agents} />
          <button
            type="button"
            disabled={!n}
            onClick={() =>
              void run({ type: "skill_install", from: p.source, agents: picked }, () => {
                onInstalled(p.name);
              })
            }
            className="mt-6 h-11 w-full rounded-full bg-ink font-semibold text-bg active:scale-[0.98] disabled:opacity-40"
          >
            {n ? t.installAndTurnOn(n) : t.pickAgent}
          </button>
        </>
      ) : null}
    </DetailSheet>
  );
}
