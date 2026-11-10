import type { AgentSummary } from "../ipc/bindings/AgentSummary";
import type { Item } from "../ipc/bindings/Item";
import { AgentLogo } from "./AgentLogo";
import { FChip } from "./FChip";
import { t } from "../strings";

/** One chip per agent that has at least one of `items`; a second click clears it. */
export function AgentFilter({ agents, items, value, onChange }: { agents: AgentSummary[]; items: Item[]; value: string | null; onChange: (key: string | null) => void }) {
  const shown = agents.filter((a) => items.some((i) => i.agents.includes(a.key)));
  if (!shown.length) return null;
  return (
    <>
      <span className="mx-1 h-5 w-px bg-rule" />
      {shown.map((a) => (
        <FChip
          key={a.key}
          icon={<AgentLogo agent={a} size={14} />}
          label={a.key === "agents_shared" ? t.sharedAgents : a.display_name}
          count={items.filter((i) => i.agents.includes(a.key)).length}
          active={value === a.key}
          onClick={() => onChange(value === a.key ? null : a.key)}
        />
      ))}
    </>
  );
}
