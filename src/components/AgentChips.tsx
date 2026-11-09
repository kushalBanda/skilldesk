import { AgentLogo } from "./AgentLogo";
// "Turn on for" chips (DESIGN.md §7.14): one per skills folder, readers of a
// shared folder named after the dot, Select all / Clear.
import type { AgentSummary } from "../ipc/bindings/AgentSummary";
import type { InstallTarget } from "../ipc/bindings/InstallTarget";
import { agentName } from "../lib/agents";
import { tildePath } from "../lib/format";
import { t } from "../strings";

export function AgentChips({
  targets,
  picked,
  onChange,
  agents,
}: {
  targets: InstallTarget[];
  picked: string[];
  onChange: (keys: string[]) => void;
  agents: AgentSummary[];
}) {
  const readers = (x: InstallTarget) => x.agents.filter((a) => a !== x.key && a !== "agents_shared");
  return (
    <>
      <div className="mb-2 mt-6 flex items-center gap-3">
        <h3 className="m-0 text-[12.5px] font-semibold text-ink-3">{t.turnOnFor}</h3>
        <button type="button" onClick={() => onChange(targets.map((x) => x.key))} className="text-[12.5px] font-semibold text-accent-text hover:underline">
          {t.selectAll}
        </button>
        <button type="button" onClick={() => onChange([])} className="text-[12.5px] font-semibold text-accent-text hover:underline">
          {t.clear}
        </button>
      </div>
      <div className="flex flex-wrap gap-2">
        {targets.map((x) => {
          const on = picked.includes(x.key);
          const r = readers(x);
          return (
            <button
              key={x.key}
              type="button"
              aria-pressed={on}
              title={tildePath(x.dir)}
              onClick={() => onChange(on ? picked.filter((k) => k !== x.key) : [...picked, x.key])}
              className={[
                "flex h-8 items-center gap-1.5 rounded-full pl-2.5 pr-3 text-[13px] font-medium active:scale-[0.97]",
                on ? "bg-ink text-bg" : "bg-bg ring-1 ring-rule hover:bg-sunk",
              ].join(" ")}
            >
              <AgentLogo agent={x.key} size={15} tone="text-current" />
              {agentName(agents, x.key)}
              {r.length ? <span className={on ? "text-bg/70" : "text-ink-3"}>· {r.map((a) => agentName(agents, a)).join(", ")}</span> : null}
            </button>
          );
        })}
      </div>
    </>
  );
}

/** The clash notes shown above the chips. */
export function ClashNotes({ name, inLibrary, targets, picked, agents }: { name: string; inLibrary: boolean; targets: InstallTarget[]; picked: string[]; agents: AgentSummary[] }) {
  const label = (x: InstallTarget) => agentName(agents, x.key);
  const clashes = targets.filter((x) => picked.includes(x.key) && x.existing.kind === "folder");
  const linkClashes = targets.filter((x) => picked.includes(x.key) && x.existing.kind === "link");
  return (
    <>
      {inLibrary ? <p className="mt-3 rounded-row bg-sunk px-3 py-2 text-[13px] text-warn-text">{t.alreadyInLibrary(name)}</p> : null}
      {clashes.length ? <p className="mt-3 rounded-row bg-sunk px-3 py-2 text-[13px] text-warn-text">{t.clashNote(clashes.map(label).join(", "), name)}</p> : null}
      {linkClashes.length ? <p className="mt-3 rounded-row bg-sunk px-3 py-2 text-[13px] text-ink-2">{t.linkClashNote(linkClashes.map(label).join(", "), name)}</p> : null}
    </>
  );
}
