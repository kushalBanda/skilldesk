import type { AgentKey } from "../ipc/bindings/AgentKey";
import type { AgentSummary } from "../ipc/bindings/AgentSummary";

// Two-letter monograms until brand logos land (DESIGN.md "Agent marks").
const MONO: Record<string, string> = {
  claude_code: "Cl",
  cursor: "Cu",
  codex: "Cx",
  opencode: "Oc",
  gemini_cli: "Ge",
  github_copilot: "Co",
  agents_shared: "∗",
};

export function monogram(a: AgentSummary | AgentKey): string {
  const key = typeof a === "string" ? a : a.key;
  if (MONO[key]) return MONO[key];
  const name = typeof a === "string" ? a : a.display_name;
  return name.replace(/[^A-Za-z]/g, "").slice(0, 2);
}

/** The columns a table shows one dot for: full-support agents found on this Mac. */
export function dotColumns(agents: AgentSummary[]): AgentSummary[] {
  return agents.filter((a) => a.tier === "full");
}

export function agentName(agents: AgentSummary[], key: AgentKey): string {
  if (key === "agents_shared") return "~/.agents";
  return agents.find((a) => a.key === key)?.display_name ?? key;
}
