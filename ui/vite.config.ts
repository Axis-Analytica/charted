import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri expects a fixed dev port and must not have Vite clear its output.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Shared folders (e.g. a VM's host mount) deliver no file-change events; poll there.
    watch: process.env.CHARTED_POLL === "1" ? { usePolling: true, interval: 300 } : undefined,
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: { target: "safari16", sourcemap: false },
  worker: { format: "es" },
});
