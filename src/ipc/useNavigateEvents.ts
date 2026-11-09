import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";

/** Tray menu → open a route in the window. */
export function useNavigateEvents() {
  const navigate = useNavigate();
  useEffect(() => {
    const un = listen<string>("navigate", (e) => navigate(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [navigate]);
}
