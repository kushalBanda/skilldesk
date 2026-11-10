import { PageHeader } from "../components/PageHeader";
import { AgentLogo } from "../components/AgentLogo";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ipc } from "../ipc/client";
import { qk, useCliStatus, useScanning, useSettings, useStartScan } from "../ipc/queries";
import { getErrorMessage } from "../ipc/error";
import { Switch } from "../components/Switch";
import type { AgentSetting } from "../ipc/bindings/AgentSetting";
import type { CliStatus } from "../ipc/bindings/CliStatus";
import { tildePath } from "../lib/format";
import { t } from "../strings";

const btn = "h-8 shrink-0 rounded-full bg-bg px-3 text-[13px] font-semibold ring-1 ring-rule hover:bg-sunk active:scale-[0.97] disabled:opacity-50";
const primary = "h-8 shrink-0 rounded-full bg-ink px-4 text-[13px] font-semibold text-bg active:scale-[0.97] disabled:opacity-50";
const field = "h-8 rounded-input bg-sunk px-3 text-[13px] text-ink outline-none placeholder:text-ink-3 focus:ring-1 focus:ring-accent";
const h2 = "m-0 text-[20px] font-bold tracking-[-0.02em]";

export function Settings() {
  const q = useSettings();
  const s = q.data;
  const [error, setError] = useState<string | null>(null);
  return (
    <div className="h-full overflow-auto page">
      <div className="flex w-full flex-col gap-10">
        <PageHeader title={t.settingsTitle} />
        {q.error || error ? (
          <p role="alert" className="m-0 text-danger">
            {error ?? getErrorMessage(q.error, t.loadFailed)}
          </p>
        ) : null}
        {s ? (
          <>
            <Agents agents={s.agents} onError={setError} />
            <Custom keys={s.custom.map((c) => c.key)} agents={s.agents} onError={setError} />
            <Cli onError={setError} />
          </>
        ) : null}
      </div>
    </div>
  );
}

function Mark({ agent }: { agent: string }) {
  return <AgentLogo agent={agent} size={16} />;
}

function Agents({ agents, onError }: { agents: AgentSetting[]; onError: (e: string | null) => void }) {
  const client = useQueryClient();
  const scan = useStartScan();
  const scanning = useScanning();
  const [busy, setBusy] = useState<string | null>(null);
  const found = agents.filter((a) => a.found);
  const missing = agents.filter((a) => !a.found);
  const toggle = async (a: AgentSetting, on: boolean) => {
    onError(null);
    setBusy(a.agent);
    try {
      await ipc.agentSetOn(a.agent, on);
      await client.invalidateQueries({ queryKey: qk.settings });
      scan();
    } catch (e) {
      onError(getErrorMessage(e, t.loadFailed));
    } finally {
      setBusy(null);
    }
  };
  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between gap-4">
        <h2 className={h2}>{t.agentsTitle}</h2>
        <span className="text-[13px] text-ink-2">{scanning ? t.rescanning : t.agentsFound(found.length, agents.length)}</span>
      </div>
      <p className="m-0 text-[13px] text-ink-2">{t.agentsLead}</p>
      <ul className="m-0 flex list-none flex-col p-0">
        {found.map((a) => (
          <AgentRow key={a.agent} a={a} control={<Switch on={a.on} label={t.readAgent(a.name)} disabled={busy !== null} onChange={(on) => void toggle(a, on)} />} />
        ))}
      </ul>
      {missing.length ? (
        <details>
          <summary className="cursor-default py-2 text-[13px] font-semibold text-ink-2">{t.notOnMac(missing.length)}</summary>
          <ul className="m-0 flex list-none flex-col p-0 opacity-70">
            {missing.map((a) => (
              <AgentRow key={a.agent} a={a} />
            ))}
          </ul>
        </details>
      ) : null}
    </section>
  );
}

function AgentRow({ a, control }: { a: AgentSetting; control?: React.ReactNode }) {
  return (
    <li className="grid min-h-11 grid-cols-[16px_minmax(110px,200px)_minmax(0,1fr)_auto_auto_auto] items-center gap-x-4 border-t border-rule-2 py-2 first:border-rule">
      <Mark agent={a.agent} />
      <span className="truncate text-[14px] font-semibold">{a.name}</span>
      <span className="min-w-0 truncate font-mono text-[12px] text-ink-2" title={[a.skills_dir, ...a.also_reads].join("\n")}>
        {tildePath(a.skills_dir)}
        {a.also_reads.length ? <span className="text-ink-3"> · {t.alsoReads(a.also_reads.map(tildePath).join(", "))}</span> : null}
      </span>
      <span className={["tnum w-[72px] text-right text-[13px]", a.on ? "text-ink-2" : "text-ink-3"].join(" ")}>{a.found ? (a.on ? t.skillsN(a.skills) : "—") : ""}</span>
      <span className="w-[76px] shrink-0 rounded-full bg-sunk px-2 text-center text-[11.5px] font-semibold leading-5 text-ink-2">{a.tier === "full" ? t.tierFull : t.tierSkills}</span>
      {control ?? <span className="w-[42px]" />}
    </li>
  );
}

function Custom({ keys, agents, onError }: { keys: string[]; agents: AgentSetting[]; onError: (e: string | null) => void }) {
  const client = useQueryClient();
  const scan = useStartScan();
  const [name, setName] = useState("");
  const [dir, setDir] = useState<string | null>(null);
  const [project, setProject] = useState("");
  const [busy, setBusy] = useState(false);
  const mine = agents.filter((a) => keys.includes(a.agent));
  const run = async (f: () => Promise<unknown>) => {
    onError(null);
    setBusy(true);
    try {
      await f();
      await client.invalidateQueries({ queryKey: qk.settings });
      scan();
      return true;
    } catch (e) {
      onError(getErrorMessage(e, t.loadFailed));
      return false;
    } finally {
      setBusy(false);
    }
  };
  const add = async () => {
    if (!dir) return;
    const ok = await run(() => ipc.customAgentSave({ key: null, name, skillsDir: dir, projectDir: project.trim() || null }));
    if (ok) {
      setName("");
      setDir(null);
      setProject("");
    }
  };
  return (
    <section className="flex flex-col gap-2">
      <h2 className={h2}>{t.customTitle}</h2>
      <p className="m-0 text-[13px] text-ink-2">{t.customLead}</p>
      {mine.length ? (
        <ul className="m-0 flex list-none flex-col p-0">
          {mine.map((a) => (
            <AgentRow
              key={a.agent}
              a={a}
              control={
                <button type="button" className={btn} disabled={busy} title={t.customRemoveNote} onClick={() => void run(() => ipc.customAgentDelete(a.agent))}>
                  {t.customRemove}
                </button>
              }
            />
          ))}
        </ul>
      ) : null}
      <form
        className="flex flex-wrap items-end gap-3 border-t border-rule pt-3"
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <label className="flex flex-col gap-1 text-[12.5px] font-semibold text-ink-3">
          {t.customName}
          <input value={name} onChange={(e) => setName(e.target.value)} maxLength={40} className={`${field} w-44 font-normal`} />
        </label>
        <div className="flex flex-col gap-1 text-[12.5px] font-semibold text-ink-3">
          {t.customFolder}
          <div className="flex h-8 items-center gap-2">
            <button type="button" className={btn} disabled={busy} onClick={() => ipc.pickFolder().then((p) => p && setDir(p), (e) => onError(getErrorMessage(e, t.loadFailed)))}>
              {t.customChoose}
            </button>
            <span className={["w-[240px] truncate font-mono text-[12px] font-normal", dir ? "text-ink" : "text-ink-3"].join(" ")} title={dir ?? undefined}>
              {dir ? tildePath(dir) : t.customNoFolder}
            </span>
          </div>
        </div>
        <label className="flex flex-col gap-1 text-[12.5px] font-semibold text-ink-3" title={t.customProjectHint}>
          {t.customProject}
          <input value={project} onChange={(e) => setProject(e.target.value)} placeholder=".myagent/skills" className={`${field} w-44 font-mono font-normal`} />
        </label>
        <button type="submit" className={primary} disabled={busy || !name.trim() || !dir}>
          {t.customAdd}
        </button>
      </form>
      <p className="m-0 text-[12.5px] text-ink-3">{t.customProjectHint} {t.customRemoveNote}</p>
    </section>
  );
}

function Cli({ onError }: { onError: (e: string | null) => void }) {
  const q = useCliStatus();
  const client = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const c = q.data;
  const install = async () => {
    onError(null);
    setBusy(true);
    try {
      client.setQueryData<CliStatus>(qk.cli, await ipc.cliInstall());
    } catch (e) {
      onError(getErrorMessage(e, t.loadFailed));
    } finally {
      setBusy(false);
    }
  };
  if (!c) return null;
  const tone = c.state === "installed" ? "bg-ok-soft text-ok" : c.state === "other" ? "bg-danger-soft text-danger" : "bg-sunk text-ink-2";
  return (
    <section className="flex min-w-0 flex-col gap-2">
      <div className="flex items-center gap-3">
        <h2 className={h2}>{t.cliTitle}</h2>
        <span className={`rounded-full px-2 text-[11.5px] font-semibold leading-5 ${tone}`}>{t.cliState[c.state]}</span>
      </div>
      <p className="m-0 text-[13px] text-ink-2">{t.cliLead}</p>
      <div className="flex items-center gap-4 border-t border-rule py-3">
        <code className="selectable min-w-0 flex-1 truncate font-mono text-[13px]">{tildePath(c.path)}</code>
        {c.state !== "installed" ? (
          <button type="button" className={primary} disabled={busy || !c.bundled} title={c.bundled ? undefined : t.cliNotBundled} onClick={() => void install()}>
            {c.state === "outdated" ? t.cliUpdate : t.cliInstall}
          </button>
        ) : null}
      </div>
      {!c.bundled ? <p className="m-0 text-[12.5px] text-ink-3">{t.cliNotBundled}</p> : null}
      {c.state === "other" ? <p className="m-0 text-[12.5px] text-ink-3">{t.cliReplaceNote}</p> : null}
      {c.state !== "not_installed" ? (
        c.on_path ? (
          <p className="m-0 text-[12.5px] text-ink-3">{t.cliOnPath}</p>
        ) : (
          <div className="flex flex-col gap-1.5">
            <p className="m-0 text-[13px] text-ink-2">{t.cliPath}</p>
            <div className="flex items-center gap-3">
              <code className="selectable min-w-0 flex-1 truncate rounded-input bg-sunk px-3 py-1.5 font-mono text-[12.5px]">{c.path_line}</code>
              <button
                type="button"
                className={btn}
                onClick={() =>
                  navigator.clipboard.writeText(c.path_line).then(
                    () => setCopied(true),
                    () => setCopied(false),
                  )
                }
              >
                {copied ? t.copied : t.copyLine}
              </button>
            </div>
          </div>
        )
      ) : null}
      <div className="flex items-baseline gap-3 pt-1">
        <code className="selectable rounded-input bg-sunk px-2 py-1 font-mono text-[12.5px]">skilldesk scan --json</code>
        <span className="text-[13px] text-ink-2">{t.cliExample}</span>
      </div>
    </section>
  );
}
