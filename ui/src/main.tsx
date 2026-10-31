import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./design/base.css";
import "./kit/kit.css";
import "./app/shell.css";
import { loadFonts } from "./design/fonts";
import { useApp } from "./state/app";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

// The shell asks before closing a window with unsaved work; App shows the dialog.
if (isTauri()) void listen("close-requested", () => window.dispatchEvent(new Event("charted:close-requested")));

// The user's fonts load as the core lists them (when added, and with a project's own).
useApp.subscribe((s, prev) => {
  if (s.state?.fonts !== prev.state?.fonts) void loadFonts(s.state?.fonts ?? []);
});

// `?spike` opens the WebKitGTK performance spike page (development only).
const spike = import.meta.env.DEV && (new URLSearchParams(location.search).has("spike") || import.meta.env.VITE_SPIKE === "1");
const view = spike ? import("./spike/Spike").then((m) => <m.Spike />) : Promise.resolve(<App />);
view.then((el) => createRoot(root).render(<StrictMode>{el}</StrictMode>));
