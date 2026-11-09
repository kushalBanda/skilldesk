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

/** Fixed dark-mode hexes of the same series, for the Wrapped card (always dark, drawn on a canvas). */
const SERIES_DARK: Record<string, string> = {
  claude_code: "#3987e5",
  cursor: "#d95926",
  codex: "#199e70",
  opencode: "#c98500",
  github_copilot: "#d55181",
};
export const OTHER_DARK = "#5c5c5c";
export const agentColorDark = (key: string) => SERIES_DARK[key] ?? OTHER_DARK;
