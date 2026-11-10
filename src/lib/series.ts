/** Color follows the agent, never its rank (dataviz palette in tokens.css). */
const SERIES: Record<string, string> = {
  claude_code: "var(--series-1)",
  cursor: "var(--series-2)",
  codex: "var(--series-3)",
  opencode: "var(--series-4)",
  github_copilot: "var(--series-5)",
};
export const OTHER_COLOR = "var(--series-other)";
export const agentColor = (key: string) => SERIES[key] ?? OTHER_COLOR;

