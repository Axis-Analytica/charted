import { defineConfig } from "@playwright/test";
import { tmpdir } from "node:os";
import { join } from "node:path";

// End-to-end tests run the real UI in Chromium against the real Rust core through
// `charted-headless serve` (the development WebSocket server), with a throwaway
// documents folder so tests never touch the user's files.
const tmp = process.env.CHARTED_E2E_TMP ?? (process.platform === "win32" ? join(tmpdir(), "charted-e2e") : "/tmp/charted-e2e");
// The specs read it too.
process.env.CHARTED_E2E_TMP = tmp;
const bin = process.env.CHARTED_BIN ?? `${process.env.CARGO_TARGET_DIR ?? "../target"}/debug/charted-headless`;
// The in-app agent's requests go to a scripted stand-in, never a real agent.
const agent = join(process.cwd(), "e2e/fake-agent.mjs");
// Ports can be moved so that several checkouts run their tests at once.
const core = Number(process.env.CHARTED_E2E_PORT ?? 7421);
const web = Number(process.env.CHARTED_E2E_WEB_PORT ?? 1421);

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  // CI's macOS and Windows machines are slower; a test that fails there gets a second
  // try (reported as flaky). Linux stays strict.
  retries: Number(process.env.CHARTED_E2E_RETRIES ?? 0),
  reporter: [["list"]],
  use: {
    baseURL: `http://localhost:${web}`,
    viewport: { width: 1600, height: 1000 },
    launchOptions: process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : undefined,
  },
  webServer: [
    {
      // e2e/server.mjs: a throwaway home, documents and state, and the stand-in agent.
      command: "node e2e/server.mjs",
      env: { CHARTED_E2E_TMP: tmp, CHARTED_E2E_BIN: bin, CHARTED_E2E_PORT: String(core), CHARTED_E2E_AGENT: agent },
      port: core,
      reuseExistingServer: false,
    },
    {
      command: `npx vite --port ${web} --strictPort`,
      env: { VITE_CHARTED_WS: `ws://127.0.0.1:${core}` },
      port: web,
      reuseExistingServer: false,
    },
  ],
});
