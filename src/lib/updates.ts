import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** Off switch for the check on launch. Per Mac, so it lives in the webview's storage. */
const AUTO_KEY = "skilldesk.updates.auto";
const key = ["app-update"] as const;

export function autoCheckOn(): boolean {
  try {
    return localStorage.getItem(AUTO_KEY) !== "off";
  } catch {
    return true;
  }
}

function setAutoCheck(on: boolean) {
  try {
    localStorage.setItem(AUTO_KEY, on ? "on" : "off");
  } catch {
    // Storage blocked: the switch resets on the next launch.
  }
}

/**
 * The newer release on GitHub, or null. Asks once per launch when the check is
 * on (never in a dev build); `checkNow` asks again on demand.
 */
export function useAppUpdate() {
  const client = useQueryClient();
  const [auto, setAuto] = useState(autoCheckOn);
  const q = useQuery<Update | null>({
    queryKey: key,
    queryFn: () => check(),
    enabled: auto && !import.meta.env.DEV,
    staleTime: Infinity,
    retry: false,
  });
  return {
    update: q.data ?? null,
    checked: q.isSuccess,
    checking: q.isFetching,
    error: q.error,
    auto,
    setAuto: (on: boolean) => {
      setAutoCheck(on);
      setAuto(on);
    },
    checkNow: () => client.fetchQuery({ queryKey: key, queryFn: () => check(), staleTime: 0 }),
  };
}

/** Downloads, verifies the signature, installs, then restarts SkillDesk. */
export async function installAndRestart(update: Update, onProgress: (pct: number | null) => void) {
  let total = 0;
  let done = 0;
  await update.downloadAndInstall((e) => {
    if (e.event === "Started") total = e.data.contentLength ?? 0;
    if (e.event === "Progress") {
      done += e.data.chunkLength;
      onProgress(total ? Math.min(100, Math.round((done / total) * 100)) : null);
    }
    if (e.event === "Finished") onProgress(null);
  });
  await relaunch();
}
