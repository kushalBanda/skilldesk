import { StatTiles } from "../components/StatTiles";
import { PageHeader } from "../components/PageHeader";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { useFixes, qk } from "../ipc/queries";
import { ipc } from "../ipc/client";
import { getErrorMessage } from "../ipc/error";
import { useAction } from "../actions/ActionProvider";
import type { Fix } from "../ipc/bindings/Fix";
import { t } from "../strings";

// DESIGN §7.2 groups: urgent · slow or gone · out of sync · unused.
const CATEGORY: Record<string, { label: string; tone: string }> = {
  broken_hooks: { label: "Urgent", tone: "bg-danger-soft text-danger" },
  slow_hook: { label: "Slow", tone: "bg-sunk text-warn-text" },
  dangling_links: { label: "Out of sync", tone: "bg-sunk text-warn-text" },
  never_run_skills: { label: "Unused", tone: "bg-sunk text-ink-3" },
  ghost_plugins: { label: "Gone", tone: "bg-sunk text-warn-text" },
  unused_marketplaces: { label: "Unused", tone: "bg-sunk text-ink-3" },
  never_called_mcp: { label: "Unused", tone: "bg-sunk text-ink-3" },
};
export const urgency = (rule: string) => CATEGORY[rule] ?? { label: "Check", tone: "bg-sunk text-ink-3" };

/** Ranked fix cards (DESIGN §7.2). Cards carry no file paths. */
export function Fixes() {
  const { data, error, isPending } = useFixes();
  const [group, setGroup] = useState<string | null>(null);
  const all = data ?? [];
  const groups = ["Urgent", "Out of sync", "Slow", "Gone", "Unused", "Check"]
    .map((g) => ({ g, n: all.filter((f) => urgency(f.rule).label === g).length }))
    .filter((x) => x.n > 0);
  const shown = group ? all.filter((f) => urgency(f.rule).label === group) : all;
  const oneClick = all.filter((f) => f.action).length;
  return (
    <section className="flex w-full max-w-[1680px] flex-col gap-6">
      <PageHeader title={t.fixes} lead={all.length ? t.fixesLead(all.length, oneClick) : undefined} />
      {error ? <p role="alert" className="text-danger">{getErrorMessage(error, t.loadFailed)}</p> : null}
      {isPending ? null : all.length === 0 ? (
        <p className="m-0 rounded-card bg-panel px-5 py-4 text-ink-2">{t.fixesEmpty}</p>
      ) : (
        <>
          <StatTiles
            label={t.fixes}
            min={150}
            items={groups.map(({ g, n }) => ({
              label: g,
              value: String(n),
              tone: g === "Urgent" ? ("danger" as const) : g === "Unused" || g === "Check" ? null : ("warn" as const),
              hint: t.fixGroupHint[g] ?? "",
              onClick: () => setGroup(group === g ? null : g),
              active: group === g,
            }))}
          />
          <ul className="m-0 grid list-none gap-4 p-0" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(380px, 1fr))" }}>
            {shown.map((f) => (
              <FixCard key={f.id} fix={f} />
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

function FixCard({ fix }: { fix: Fix }) {
  const navigate = useNavigate();
  const client = useQueryClient();
  const { run } = useAction();
  const u = urgency(fix.rule);
  return (
    <li data-row-id={fix.id} className="flex min-w-0 flex-col rounded-card bg-panel px-5 py-5 ring-1 ring-rule-2 transition-shadow duration-150 hover:ring-rule">
      <span className={["self-start rounded-full px-2 py-0.5 text-[11.5px] font-medium", u.tone].join(" ")}>{u.label}</span>
      <h2 className="mb-1.5 mt-3 text-[20px] font-bold leading-tight tracking-[-0.03em]">{fix.title}</h2>
      <p className="m-0 line-clamp-3 text-[13.5px] text-ink-2" title={fix.why}>
        {fix.why}
      </p>
      <div className="mt-auto flex items-center gap-2 pt-5">
        {fix.action ? (
          <button
            type="button"
            onClick={() => void run(fix.action!)}
            className="h-9 rounded-full bg-ink px-4 text-[13.5px] font-semibold text-bg active:scale-[0.97]"
          >
            {fix.action_label}
          </button>
        ) : null}
        <button
          type="button"
          onClick={() => navigate(fix.link)}
          className={[
            "h-9 rounded-full px-4 text-[13.5px] font-semibold active:scale-[0.97]",
            fix.action ? "bg-bg ring-1 ring-rule hover:bg-sunk" : "bg-ink text-bg",
          ].join(" ")}
        >
          {fix.action ? t.showMe : fix.action_label}
        </button>
        <button
          type="button"
          onClick={async () => {
            await ipc.fixDismiss(fix.id);
            await client.invalidateQueries({ queryKey: qk.fixes });
          }}
          className="ml-auto h-9 rounded-full px-3 text-[13px] font-medium text-ink-3 hover:bg-sunk hover:text-ink-2"
        >
          {t.dismiss}
        </button>
      </div>
    </li>
  );
}
