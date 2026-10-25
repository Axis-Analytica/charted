import { defineConfig } from "@playwright/test";

// End-to-end tests run the real UI in Chromium against the real Rust core through
// `charted-headless serve` (the development WebSocket server), with a throwaway
// documents folder so tests never touch the user's files.
const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const bin = process.env.CHARTED_BIN ?? "../target/debug/charted-headless";
const theme = `${tmp}/home/.local/state/omarchy/current/theme`;

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:1421",
    viewport: { width: 1600, height: 1000 },
    launchOptions: process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : undefined,
  },
  webServer: [
    {
      // HOME is a copy holding the desktop's Omarchy theme, so tests can switch it.
      command: `rm -rf ${tmp} && mkdir -p ${tmp}/docs ${tmp}/state ${theme} && (cp -L ${process.env.HOME}/.local/state/omarchy/current/theme/* ${theme}/ 2>/dev/null; cp -L ${process.env.HOME}/.local/state/omarchy/current/theme.name ${theme}/../ 2>/dev/null; true) && HOME=${tmp}/home XDG_DOCUMENTS_DIR=${tmp}/docs ${bin} serve --port 7421 --state-dir ${tmp}/state`,
      port: 7421,
      reuseExistingServer: false,
    },
    {
      command: "VITE_CHARTED_WS=ws://127.0.0.1:7421 npx vite --port 1421 --strictPort",
      port: 1421,
      reuseExistingServer: false,
    },
  ],
});
