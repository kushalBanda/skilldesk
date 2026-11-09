import { useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "./client";
import type { ItemKind } from "./bindings/ItemKind";

export const qk = {
  overview: ["overview"] as const,
  items: (kind: ItemKind) => ["items", kind] as const,
  history: ["history"] as const,
  fixes: ["fixes"] as const,
  sessions: ["sessions"] as const,
  secrets: ["secrets"] as const,
  storage: ["storage"] as const,
  scanning: ["scanning"] as const,
  wrapped: (year: number | null) => ["wrapped", year] as const,
  settings: ["settings"] as const,
  cli: ["cli"] as const,
};

/** True while a scan runs (set on start, cleared by the last `scan-progress`). */
export function useScanning(): boolean {
  return useQuery({ queryKey: qk.scanning, queryFn: () => false, initialData: false, staleTime: Infinity }).data;
}

export function useStartScan() {
  const client = useQueryClient();
  return () => {
    client.setQueryData(qk.scanning, true);
    ipc.scanStart().catch((err) => {
      client.setQueryData(qk.scanning, false);
      console.error("scan_start failed", err);
    });
  };
}

export function useStorage() {
  return useQuery({ queryKey: qk.storage, queryFn: ipc.storageGet });
}

export function useWrapped(year: number | null) {
  return useQuery({ queryKey: qk.wrapped(year), queryFn: () => ipc.wrappedData(year) });
}

export function useSettings() {
  return useQuery({ queryKey: qk.settings, queryFn: ipc.settingsGet });
}

export function useCliStatus() {
  return useQuery({ queryKey: qk.cli, queryFn: ipc.cliStatus });
}

export function useSecrets() {
  return useQuery({ queryKey: qk.secrets, queryFn: ipc.secretsList });
}

export function useSessions() {
  return useQuery({ queryKey: qk.sessions, queryFn: ipc.sessionsList });
}

export function useFixes() {
  return useQuery({ queryKey: qk.fixes, queryFn: ipc.fixesList });
}

export function useHistory() {
  return useQuery({ queryKey: qk.history, queryFn: ipc.historyList });
}

export function useOverview() {
  return useQuery({ queryKey: qk.overview, queryFn: ipc.overview });
}

export function useItems(kind: ItemKind) {
  return useQuery({ queryKey: qk.items(kind), queryFn: () => ipc.itemsList(kind) });
}

/**
 * Start one scan per app launch (not on the first run: Welcome asks first)
 * and refetch only the kinds a scan stage reports.
 */
export function useScanOnLaunch(scannedBefore: boolean | undefined) {
  const client = useQueryClient();
  const start = useStartScan();
  const started = useRef(false);
  // Decided once, on the first known answer: a first run waits for Welcome.
  useEffect(() => {
    if (scannedBefore === undefined || started.current) return;
    started.current = true;
    if (scannedBefore) start();
  }, [scannedBefore, start]);
  useEffect(() => {
    const unlistenProgress = listen<{ done: boolean }>("scan-progress", (e) => {
      if (e.payload.done) client.setQueryData(qk.scanning, false);
    });
    const unlisten = listen<{ kinds: ItemKind[] }>("inventory-changed", (e) => {
      client.invalidateQueries({ queryKey: qk.overview });
      client.invalidateQueries({ queryKey: ["wrapped"] });
      client.invalidateQueries({ queryKey: qk.settings });
      for (const kind of e.payload.kinds) client.invalidateQueries({ queryKey: qk.items(kind) });
      if (e.payload.kinds.includes("secret")) client.invalidateQueries({ queryKey: qk.secrets });
      if (e.payload.kinds.includes("storage")) {
        client.invalidateQueries({ queryKey: qk.storage });
        client.invalidateQueries({ queryKey: qk.fixes });
      }
      if (e.payload.kinds.includes("session")) {
        client.invalidateQueries({ queryKey: qk.sessions });
        client.invalidateQueries({ queryKey: ["sessionSearch"] });
      }
    });
    const unlistenHistory = listen("history-changed", () => {
      client.invalidateQueries({ queryKey: qk.history });
    });
    const unlistenFixes = listen("fixes-changed", () => {
      client.invalidateQueries({ queryKey: qk.fixes });
    });
    return () => {
      unlistenProgress.then((f) => f());
      unlisten.then((f) => f());
      unlistenHistory.then((f) => f());
      unlistenFixes.then((f) => f());
    };
  }, [client]);
}
