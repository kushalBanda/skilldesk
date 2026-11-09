// Online / Offline for the top bar (DESIGN.md §7.21). Set only by the
// Marketplace screen from its own requests; nothing probes the network.
import { useSyncExternalStore } from "react";

export type NetStatus = "online" | "offline" | null;
let status: NetStatus = null;
const subs = new Set<() => void>();

export function setNetStatus(s: NetStatus) {
  if (s === status) return;
  status = s;
  subs.forEach((f) => f());
}

export function useNetStatus(): NetStatus {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => status,
  );
}
