import { defineConfig } from "@playwright/test";

// End-to-end tests run the real UI in Chromium against the real Rust core through
// `charted-headless serve` (the development WebSocket server), with a throwaway
// documents folder so tests never touch the user's files.
const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const bin = process.env.CHARTED_BIN ?? `${process.env.CARGO_TARGET_DIR ?? "../target"}/debug/charted-headless`;
const theme = `${tmp}/home/.local/state/omarchy/current/theme`;
// Ports can be moved so that several checkouts run their tests at once.
const core = Number(process.env.CHARTED_E2E_PORT ?? 7421);
const web = Number(process.env.CHARTED_E2E_WEB_PORT ?? 1421);

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: `http://localhost:${web}`,
    viewport: { width: 1600, height: 1000 },
    launchOptions: process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : undefined,
  },
  webServer: [
    {
      // HOME is a copy holding the desktop's Omarchy theme, so tests can switch it. The
      // desktop's Hyprland is hidden, so motion is on as on CI (Hyprland may turn it off).
      command: `rm -rf ${tmp} && mkdir -p ${tmp}/docs ${tmp}/state ${theme} && (cp -L ${process.env.HOME}/.local/state/omarchy/current/theme/* ${theme}/ 2>/dev/null; cp -L ${process.env.HOME}/.local/state/omarchy/current/theme.name ${theme}/../ 2>/dev/null; true) && env -u HYPRLAND_INSTANCE_SIGNATURE HOME=${tmp}/home XDG_DOCUMENTS_DIR=${tmp}/docs ${bin} serve --port ${core} --state-dir ${tmp}/state`,
      port: core,
      reuseExistingServer: false,
    },
    {
      command: `VITE_CHARTED_WS=ws://127.0.0.1:${core} npx vite --port ${web} --strictPort`,
      port: web,
      reuseExistingServer: false,
    },
  ],
});
