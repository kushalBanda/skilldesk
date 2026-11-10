// Every in-app link goes to the exact thing it names: an item opens its own
// sheet, a count opens the list already filtered to that count. A link that
// can only reach a whole screen is not a link.
import type { Item } from "../ipc/bindings/Item";

/** Query string from the set values only: `{ status: "unused", agent: null }` → `?status=unused`. */
export function withParams(path: string, params: Record<string, string | null | undefined>): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v) q.set(k, v);
  const s = q.toString();
  return s ? `${path}?${s}` : path;
}

/** The screen and sheet of one item. */
export function itemRoute(item: Pick<Item, "id" | "kind">): string {
  const id = item.id;
  switch (item.kind) {
    case "skill":
      return withParams("/skills", { id });
    case "hook":
      return withParams("/hooks", { id });
    case "mcp_server":
      return withParams("/mcp", { id });
    case "plugin":
      return withParams("/plugins", { id });
    case "subagent":
      return withParams("/subagents", { id });
    case "command":
      return withParams("/subagents", { tab: "command", id });
    case "memory":
      return withParams("/memory", { id });
    default:
      return "/";
  }
}
