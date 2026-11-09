// ⌘K: jump to a screen, find any item, run an action (DESIGN.md §3, §5).
// Actions go through the same plan → confirm → apply → Undo path as a click.
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useNavigate } from "react-router-dom";
import { useAction } from "../actions/ActionProvider";
import { useFixes, useItems, useOverview, useSessions, useStartScan } from "../ipc/queries";
import type { ActionRequest } from "../ipc/bindings/ActionRequest";
import type { Item } from "../ipc/bindings/Item";
import type { ItemKind } from "../ipc/bindings/ItemKind";
import { agentName } from "../lib/agents";
import { kindLabel, nav, t } from "../strings";

type Group = "action" | "jump" | "item";
type Entry = {
  key: string;
  group: Group;
  label: string;
  sub?: string;
  /** Extra words the query may match (not shown). */
  words?: string;
  run: () => void;
};

const SEARCHED: ItemKind[] = ["skill", "hook", "mcp_server", "plugin", "subagent", "command", "memory"];

function route(item: Item): string {
  const id = encodeURIComponent(item.id);
  switch (item.kind) {
    case "skill":
      return `/skills?id=${id}`;
    case "hook":
      return `/hooks?id=${id}`;
    case "mcp_server":
      return `/mcp?id=${id}`;
    case "plugin":
      return `/plugins?id=${id}`;
    case "subagent":
      return `/subagents?id=${id}`;
    case "command":
      return `/subagents?tab=command&id=${id}`;
    case "memory":
      return `/memory?id=${id}`;
    default:
      return "/";
  }
}

/** Every query word must appear; a name that starts with the query ranks first. */
function score(e: Entry, q: string, words: string[]): number | null {
  const label = e.label.toLowerCase();
  const hay = `${label} ${e.sub ?? ""} ${e.words ?? ""}`.toLowerCase();
  if (!words.every((w) => hay.includes(w))) return null;
  if (label.startsWith(q)) return 0;
  if (label.includes(q)) return 1;
  return 2;
}

const LIMIT: Record<Group, number> = { action: 8, jump: 8, item: 40 };

/** Words that ask for a change. Only then do actions lead the list, so ↩ after
 *  a name opens that item and never changes it by surprise. */
const VERBS = ["disable", "enable", "turn", "fix", "rescan", "scan", "remove", "switch"];
function asksForAction(words: string[]): boolean {
  const w = words[0];
  return !!w && w.length >= 3 && VERBS.some((v) => v.startsWith(w));
}

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const navigate = useNavigate();
  const { run } = useAction();
  const startScan = useStartScan();
  const overview = useOverview();
  const fixes = useFixes();
  const sessions = useSessions();
  // SEARCHED is a fixed list, so the hook order never changes.
  const kinds = SEARCHED.map((k) => useItems(k));
  const agents = overview.data?.agents ?? [];
  const listId = useId();
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const items = kinds.flatMap((k) => k.data ?? []);
  const loaded = kinds.map((k) => k.dataUpdatedAt).join(",");
  const loading = kinds.some((k) => k.isPending);

  // Close first, then act: the plan dialog and toast must not sit under the palette.
  const act = (req: ActionRequest) => () => {
    onClose();
    void run(req);
  };
  const go = (to: string) => () => {
    onClose();
    navigate(to);
  };

  const entries = useMemo<Entry[]>(() => {
    const out: Entry[] = [];
    out.push({ key: "rescan", group: "action", label: t.palRescan, words: "scan refresh", run: () => (onClose(), startScan()) });
    for (const f of fixes.data ?? []) {
      if (!f.action) continue;
      out.push({ key: `fix:${f.id}`, group: "action", label: t.palFix(f.title), sub: f.action_label, words: "fix", run: act(f.action) });
    }
    for (const it of items) {
      const st = it.status.state;
      const who = it.agents.map((a) => agentName(agents, a)).join(", ");
      if (it.facts.kind === "hook" && (it.facts.switch === "stash" || it.facts.switch === "native") && st !== "gone" && st !== "unreadable") {
        const on = st === "off";
        out.push({
          key: `hook:${it.id}`,
          group: "action",
          label: on ? t.palHookOn(it.name) : t.palHookOff(it.name),
          sub: `${who} · ${it.facts.event}`,
          words: `${on ? "turn on" : "turn off"} ${it.facts.owner ?? ""} ${st}`,
          run: act({ type: "hook_set_enabled", ids: [it.id], on }),
        });
      }
      if (it.facts.kind === "plugin" && it.facts.installed && !it.facts.project) {
        const on = st !== "ok";
        out.push({
          key: `plugin:${it.id}`,
          group: "action",
          label: on ? t.palPluginOn(it.name) : t.palPluginOff(it.name),
          sub: it.facts.marketplace,
          words: on ? "turn on" : "turn off",
          run: act({ type: "plugin_set_enabled", id: it.id, on }),
        });
      }
      if (it.facts.kind === "mcp_server") {
        for (const l of it.facts.locations) {
          if (l.switch !== "stash" && l.switch !== "native") continue;
          const a = agentName(agents, l.agent);
          out.push({
            key: `mcp:${it.id}:${l.entry}`,
            group: "action",
            label: l.enabled ? t.palMcpOff(it.name, a) : t.palMcpOn(it.name, a),
            sub: l.transport,
            words: `server ${l.enabled ? "turn off" : "turn on"}`,
            run: act({ type: "mcp_set_enabled", id: it.id, entry: l.entry, on: !l.enabled }),
          });
        }
      }
    }
    for (const s of nav) for (const i of s.items) out.push({ key: `jump:${i.to}`, group: "jump", label: i.label, sub: s.group, run: go(i.to) });
    out.push({ key: "jump:/settings", group: "jump", label: t.palSettings, sub: "⌘,", words: "preferences", run: go("/settings") });
    for (const it of items) {
      if (it.status.state === "not_an_item") continue;
      out.push({
        key: `item:${it.id}`,
        group: "item",
        label: it.kind === "command" ? `/${it.name}` : it.name,
        sub: `${kindLabel[it.kind]} · ${it.agents.map((a) => agentName(agents, a)).join(", ")}`,
        run: go(route(it)),
      });
    }
    for (const s of sessions.data ?? []) {
      out.push({ key: `session:${s.id}`, group: "item", label: s.title, sub: `${kindLabel.session} · ${agentName(agents, s.origin)}`, run: go(`/sessions?id=${encodeURIComponent(s.id)}`) });
    }
    return out;
    // `loaded` stands for the item lists (new data, new stamp).
  }, [loaded, fixes.data, sessions.data, agents]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const words = q.split(/\s+/).filter(Boolean);
    const by: Record<Group, { e: Entry; s: number }[]> = { action: [], jump: [], item: [] };
    for (const e of entries) {
      if (!q) {
        // Empty query: a short list of actions and every screen; items wait for words.
        if (e.group === "jump" || e.key === "rescan" || e.key.startsWith("fix:")) by[e.group].push({ e, s: 0 });
        continue;
      }
      // Sessions are many; they join once the query is specific.
      if (e.key.startsWith("session:") && q.length < 3) continue;
      const s = score(e, q, words);
      if (s !== null) by[e.group].push({ e, s });
    }
    const out: { group: Group; entries: Entry[] }[] = [];
    // Empty query: Rescan leads (safe). Otherwise actions lead only on a verb.
    const order: Group[] = !q || asksForAction(words) ? ["action", "jump", "item"] : ["jump", "item", "action"];
    for (const g of order) {
      const sorted = by[g].sort((a, b) => a.s - b.s || a.e.label.length - b.e.label.length).map((x) => x.e);
      const limited = !q && g === "jump" ? sorted : sorted.slice(0, LIMIT[g]);
      if (limited.length) out.push({ group: g, entries: limited });
    }
    return out;
  }, [entries, query]);
  const flat = shown.flatMap((g) => g.entries);

  useEffect(() => setActive(0), [query]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);
  useEffect(() => input.current?.focus(), []);

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "ArrowDown" || (e.ctrlKey && e.key === "n")) {
      e.preventDefault();
      setActive((i) => Math.min(flat.length - 1, i + 1));
    } else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "p")) {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      flat[active]?.run();
    } else if (e.key === "Tab") {
      // Focus stays in the palette; the list moves instead (it is a modal task).
      e.preventDefault();
      setActive((i) => (flat.length ? (i + (e.shiftKey ? flat.length - 1 : 1)) % flat.length : 0));
    }
  };

  const heading: Record<Group, string> = { action: t.palActions, jump: t.palJump, item: t.palItems };
  let n = -1;
  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center pt-[12vh]" onKeyDown={onKey}>
      <div className="scrim absolute inset-0" data-motion onMouseDown={onClose} aria-hidden="true" />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t.paletteLabel}
        data-motion
        className="glass palette-in relative flex max-h-[60vh] w-[640px] max-w-[calc(100vw-48px)] flex-col overflow-hidden rounded-panel shadow-card"
      >
        <div className="flex items-center gap-3 border-b border-rule-2 px-5">
          <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true" className="shrink-0 text-ink-3">
            <circle cx="7" cy="7" r="4.5" />
            <path d="M10.5 10.5L14 14" />
          </svg>
          <input
            ref={input}
            role="combobox"
            aria-expanded="true"
            aria-controls={listId}
            aria-activedescendant={flat[active] ? `${listId}-${active}` : undefined}
            aria-autocomplete="list"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t.paletteHint}
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="off"
            className="h-14 flex-1 bg-transparent text-[16px] text-ink outline-none placeholder:text-ink-3"
          />
          <kbd className="rounded-[6px] px-1.5 text-[11.5px] text-ink-3 ring-1 ring-rule">esc</kbd>
        </div>
        <div ref={list} id={listId} role="listbox" aria-label={t.paletteLabel} className="flex-1 overflow-auto p-2">
          {flat.length === 0 ? <p className="m-0 px-3 py-6 text-center text-ink-3">{loading ? t.paletteLoading : t.paletteNone(query.trim())}</p> : null}
          {shown.map((g) => (
            <div key={g.group} role="group" aria-label={heading[g.group]} className="pb-1">
              <div className="px-3 pb-1 pt-2 text-[11.5px] font-semibold tracking-[0.01em] text-ink-3">{heading[g.group]}</div>
              {g.entries.map((e) => {
                n += 1;
                const i = n;
                return (
                  <div
                    key={e.key}
                    id={`${listId}-${i}`}
                    data-index={i}
                    role="option"
                    aria-selected={i === active}
                    onMouseMove={() => i !== active && setActive(i)}
                    onClick={e.run}
                    className={["flex h-10 items-center gap-3 rounded-row px-3", i === active ? "bg-accent-soft" : ""].join(" ")}
                  >
                    <span className="min-w-0 flex-1 truncate font-medium text-ink">{e.label}</span>
                    {e.sub ? <span className="max-w-[45%] shrink-0 truncate text-[12.5px] text-ink-3">{e.sub}</span> : null}
                    {i === active ? <span className="text-[12px] text-ink-3" aria-hidden="true">↩</span> : null}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
        <div className="flex items-center justify-between border-t border-rule-2 px-5 py-2 text-[11.5px] text-ink-3">
          <span>{t.palKeys}</span>
          <span>{t.palPlanned}</span>
        </div>
      </div>
    </div>
  );
}
