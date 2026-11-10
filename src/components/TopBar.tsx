import { useEffect, useState } from "react";
import { t } from "../strings";
import { useOverview, useScanning, useStartScan } from "../ipc/queries";

/** Window drag strip above the screen; scan status lives in the sidebar (ScanButton). */
export function TopBar() {
  return <div className="h-12 shrink-0" data-tauri-drag-region />;
}

/** Ticks once a minute, so "2 min ago" stays true. */
function useNow(ms = 60_000) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(id);
  }, [ms]);
  return now;
}

/** Sidebar footer: a sync glyph with a status dot. Click rescans; the tooltip says when the last scan ran. */
export function ScanButton() {
  const scannedAt = useOverview().data?.scanned_at;
  const scanning = useScanning();
  const start = useStartScan();
  const now = useNow();
  const status = scanning ? t.scanning : scannedAt ? t.scanned(t.ago(now - new Date(scannedAt).getTime())) : t.notScanned;
  const label = `${t.rescan}. ${status}`;
  return (
    <button
      type="button"
      onClick={() => start()}
      disabled={scanning}
      aria-label={label}
      title={label}
      className="relative flex h-8 w-8 shrink-0 items-center justify-center rounded-input text-ink-3 transition-colors duration-150 hover:bg-sunk hover:text-ink-2 active:scale-[0.92] disabled:hover:bg-transparent"
    >
      <svg
        width="16"
        height="16"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        className={scanning ? "animate-spin" : ""}
      >
        <path d="M13.25 8A5.25 5.25 0 014.3 11.7M2.75 8a5.25 5.25 0 018.95-3.7" />
        <path d="M11.75 2v2.5h-2.5M4.25 14v-2.5h2.5" />
      </svg>
      <i
        aria-hidden="true"
        className={[
          "absolute right-1 top-1 h-[7px] w-[7px] rounded-full ring-2 ring-sidebar",
          scanning ? "animate-pulse bg-accent" : scannedAt ? "bg-ok" : "bg-rule",
        ].join(" ")}
      />
      <span className="sr-only" aria-live="polite">
        {status}
      </span>
    </button>
  );
}
