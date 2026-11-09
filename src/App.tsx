import type React from "react";
import { HashRouter, Route, Routes } from "react-router-dom";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { useOverview, useScanOnLaunch, useScanning, useStartScan } from "./ipc/queries";
import { Welcome } from "./screens/Welcome";
import { Skills } from "./screens/Skills";
import { History } from "./screens/History";
import { Hooks } from "./screens/Hooks";
import { Fixes } from "./screens/Fixes";
import { Plugins } from "./screens/Plugins";
import { Mcp } from "./screens/Mcp";
import { useNavigateEvents } from "./ipc/useNavigateEvents";
import { ActionProvider } from "./actions/ActionProvider";
import { Dashboard } from "./screens/Dashboard";
import { Placeholder } from "./screens/Placeholder";
import { Adopt } from "./screens/Adopt";
import { Marketplace } from "./screens/Marketplace";
import { Subagents } from "./screens/Subagents";
import { Memory } from "./screens/Memory";
import { Sessions } from "./screens/Sessions";
import { Secrets } from "./screens/Secrets";
import { Cleanup } from "./screens/Cleanup";
import { Wrapped } from "./screens/Wrapped";
import { Settings } from "./screens/Settings";
import { useEffect, useState } from "react";
import { CommandPalette } from "./components/CommandPalette";
import { ipc } from "./ipc/client";
import { useLocation, useNavigate } from "react-router-dom";
import { enterScreen } from "./lib/motion";
import { nav } from "./strings";
import { useTrackBack } from "./components/BackLink";

export function App() {
  const overview = useOverview();
  const scanning = useScanning();
  const startScan = useStartScan();
  useScanOnLaunch(overview.isSuccess ? overview.data.scanned_at !== null : undefined);
  const built = new Set(["/", "/fixes", "/skills", "/plugins", "/hooks", "/mcp", "/history", "/marketplace", "/subagents", "/memory", "/sessions", "/secrets", "/cleanup", "/wrapped", "/settings"]);
  const others = nav.flatMap((s) => s.items).filter((i) => !built.has(i.to));
  if (overview.isSuccess && overview.data.scanned_at === null && !scanning) {
    return <Welcome agents={overview.data.agents} known={overview.data.known_agents} onScan={startScan} />;
  }
  return (
    <HashRouter>
      <ActionProvider>
      <NavigateEvents />
      <div className="flex h-full">
        <Sidebar />
        <div className="flex min-w-0 flex-1 flex-col">
          <TopBar />
          <main className="min-h-0 flex-1 overflow-hidden">
            <Routes>
              <Route
                path="/"
                element={
                  <Padded>
                    <Dashboard data={overview.data} error={overview.error} loading={overview.isPending} />
                  </Padded>
                }
              />
              <Route path="/fixes" element={<Padded><Fixes /></Padded>} />
              <Route path="/skills" element={<Skills />} />
              <Route path="/adopt" element={<Adopt />} />
              <Route path="/marketplace" element={<Marketplace />} />
              <Route path="/hooks" element={<Hooks />} />
              <Route path="/plugins" element={<Plugins />} />
              <Route path="/mcp" element={<Mcp />} />
              <Route path="/subagents" element={<Subagents />} />
              <Route path="/memory" element={<Memory />} />
              <Route path="/sessions" element={<Sessions />} />
              <Route path="/secrets" element={<Secrets />} />
              <Route path="/cleanup" element={<Cleanup />} />
              <Route path="/wrapped" element={<Wrapped />} />
              <Route path="/settings" element={<Settings />} />
              <Route path="/history" element={<Padded><History /></Padded>} />
              {others.map((i) => (
                <Route key={i.to} path={i.to} element={<Padded><Placeholder title={i.label} /></Padded>} />
              ))}
            </Routes>
          </main>
        </div>
      </div>
      </ActionProvider>
    </HashRouter>
  );
}

function Padded({ children }: { children: React.ReactNode }) {
  return <div className="h-full overflow-auto page">{children}</div>;
}

/** Solid surfaces when macOS asks for less transparency; checked again on focus. */
function useTransparencySetting() {
  useEffect(() => {
    const check = () =>
      ipc
        .reduceTransparency()
        .then((on) => {
          if (on) document.documentElement.dataset.transparency = "reduce";
          else delete document.documentElement.dataset.transparency;
        })
        .catch(() => {});
    check();
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
  }, []);
}

/** Each new screen fades in and rises a few px (DESIGN.md "Motion"). */
function useScreenMotion() {
  const { pathname } = useLocation();
  useEffect(() => {
    enterScreen(document.querySelector("main > *"));
  }, [pathname]);
}

function NavigateEvents() {
  useTrackBack();
  useNavigateEvents();
  useScreenMotion();
  useTransparencySetting();
  const navigate = useNavigate();
  const [palette, setPalette] = useState(false);
  // ⌘, opens Settings (macOS convention); ⌘K toggles the palette.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.shiftKey || e.altKey || e.ctrlKey) return;
      if (e.key === ",") {
        e.preventDefault();
        navigate("/settings");
      } else if (e.key.toLowerCase() === "k") {
        // Not over a confirm dialog: that dialog owns the window.
        if (!palette && document.querySelector("[aria-modal='true']")) return;
        e.preventDefault();
        setPalette((v) => !v);
      }
    };
    const onOpen = () => setPalette(true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("skilldesk:palette", onOpen);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("skilldesk:palette", onOpen);
    };
  }, [navigate, palette]);
  return palette ? <PaletteHost onClose={() => setPalette(false)} /> : null;
}

/** Gives focus back to where it was before ⌘K. */
function PaletteHost({ onClose }: { onClose: () => void }) {
  const [before] = useState(() => document.activeElement as HTMLElement | null);
  useEffect(() => () => before?.focus?.({ preventScroll: true }), [before]);
  return <CommandPalette onClose={onClose} />;
}
