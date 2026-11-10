import type { ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import type { Item } from "../ipc/bindings/Item";
import type { ItemKind } from "../ipc/bindings/ItemKind";
import { itemRoute } from "../lib/routes";
import { useItems } from "../ipc/queries";
import { ipc } from "../ipc/client";
import { kindLabel, t } from "../strings";

/**
 * One part of a place line that goes somewhere: a path opens in Finder, a
 * plugin opens its sheet. Quiet underline at rest, accent on hover, so the
 * line still reads as text.
 */
export function PlaceLink({ onClick, title, children, className = "" }: { onClick: () => void; title?: string; children: ReactNode; className?: string }) {
  return (
    <button
      type="button"
      title={title}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className={[
        "max-w-full truncate rounded-[4px] text-left underline decoration-rule underline-offset-[3px] transition-colors duration-100 hover:text-accent-text hover:decoration-current active:opacity-70",
        className,
      ].join(" ")}
    >
      {children}
    </button>
  );
}

/** A path that opens in Finder. */
export function PathLink({ path, children, className }: { path: string; children: ReactNode; className?: string }) {
  return (
    <PlaceLink onClick={() => void ipc.revealInFinder(path)} title={`${t.showInFinder}: ${path}`} className={className}>
      {children}
    </PlaceLink>
  );
}

/** Plugin id (`name@marketplace`) to its Plugins sheet route; null when no such plugin is listed. */
export function usePluginRoute(): (pluginId: string) => string | null {
  const plugins = useItems("plugin").data ?? [];
  return (pluginId) => {
    const hit = plugins.find((p) => p.facts.kind === "plugin" && p.facts.id === pluginId);
    return hit ? `/plugins?id=${encodeURIComponent(hit.id)}` : null;
  };
}

/** Skill name to its Skills sheet route (same agent first); null when no such skill is listed. */
export function useSkillRoute(): (name: string, agents?: string[]) => string | null {
  const skills = useItems("skill").data ?? [];
  return (name, agents = []) => {
    const named = skills.filter((s) => s.name === name && s.status.state !== "not_an_item");
    const hit = named.find((s) => s.agents.some((a) => agents.includes(a))) ?? named[0];
    return hit ? `/skills?id=${encodeURIComponent(hit.id)}` : null;
  };
}

/** Plugin id this item came from, when it came from one. */
function pluginOf(it: Item): string | null {
  const f = it.facts;
  switch (f.kind) {
    case "skill":
      return f.locations.find((l) => l.plugin)?.plugin?.id ?? null;
    case "subagent":
    case "command":
      return f.locations.find((l) => l.plugin)?.plugin?.id ?? null;
    case "hook":
      return f.plugin?.id ?? null;
    case "mcp_server":
      return f.locations.find((l) => l.plugin)?.plugin ?? null;
    default:
      return null;
  }
}

const CONTENT_SHOWN = 12;

/**
 * What a plugin brings, by kind, each name a link to its own sheet. Renders
 * nothing when SkillDesk lists none of them (the caller keeps its count line).
 */
export function PluginContents({ pluginId }: { pluginId: string }) {
  const navigate = useNavigate();
  const lists = [
    { kind: "skill" as ItemKind, q: useItems("skill") },
    { kind: "command" as ItemKind, q: useItems("command") },
    { kind: "subagent" as ItemKind, q: useItems("subagent") },
    { kind: "hook" as ItemKind, q: useItems("hook") },
    { kind: "mcp_server" as ItemKind, q: useItems("mcp_server") },
  ];
  const groups = lists
    .map(({ kind, q }) => ({
      kind,
      items: (q.data ?? [])
        .filter((it) => it.status.state !== "not_an_item" && it.status.state !== "gone" && pluginOf(it) === pluginId)
        .sort((a, b) => a.name.localeCompare(b.name)),
    }))
    .filter((g) => g.items.length);
  if (!groups.length) return null;
  return (
    <dl className="m-0 mt-4 flex flex-col gap-2.5 text-[13px]">
      {groups.map((g) => (
        <div key={g.kind} className="grid grid-cols-[96px_minmax(0,1fr)] gap-3">
          <dt className="text-ink-3">
            {kindLabel[g.kind]} <span className="tnum">{g.items.length}</span>
          </dt>
          <dd className="m-0 flex min-w-0 flex-wrap gap-x-2.5 gap-y-1 text-ink-2">
            {g.items.slice(0, CONTENT_SHOWN).map((it) => (
              <PlaceLink key={it.id} onClick={() => navigate(itemRoute(it))} title={t.openItem(it.name)}>
                {it.kind === "command" ? `/${it.name}` : it.name}
              </PlaceLink>
            ))}
            {g.items.length > CONTENT_SHOWN ? <span className="text-ink-3">{t.andMore(g.items.length - CONTENT_SHOWN)}</span> : null}
          </dd>
        </div>
      ))}
    </dl>
  );
}

/** A plugin id that opens its Plugins sheet; plain text when that plugin is not listed. */
export function PluginName({ id }: { id: string }) {
  const navigate = useNavigate();
  const to = usePluginRoute()(id);
  return to ? (
    <PlaceLink onClick={() => navigate(to)} title={t.openPlugin}>
      {id}
    </PlaceLink>
  ) : (
    <>{id}</>
  );
}
