import { PageHeader } from "../components/PageHeader";
import { useHistory } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import { useAction } from "../actions/ActionProvider";
import { formatDate } from "../lib/format";
import { t } from "../strings";

export function History() {
  const { data, error, isPending } = useHistory();
  const { undo } = useAction();
  return (
    <section className="flex flex-col gap-6">
      <PageHeader title={t.history} />
      {error ? <p role="alert" className="text-danger">{getErrorMessage(error, t.loadFailed)}</p> : null}
      {isPending ? null : data && data.length === 0 ? (
        <p className="text-ink-2">{t.historyEmpty}</p>
      ) : (
        <ul className="m-0 flex list-none flex-col p-0">
          {data?.map((r) => (
            <li key={r.id} className="flex items-center gap-4 border-t border-rule-2 py-3">
              <div className="min-w-0 flex-1">
                <div className={["truncate font-semibold tracking-[-0.02em]", r.status === "undone" ? "text-ink-3 line-through" : ""].join(" ")}>
                  {r.summary}
                </div>
                <div className="tnum text-[13px] text-ink-3">
                  {formatDate(r.at)}, {new Date(r.at).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}
                </div>
              </div>
              {r.status === "applied" ? (
                <button
                  type="button"
                  onClick={() => void undo(r.id)}
                  className="h-9 rounded-full bg-bg px-4 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97]"
                >
                  {t.undo}
                </button>
              ) : (
                <span className="text-[13px] text-ink-3">{t.undoneLabel}</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
