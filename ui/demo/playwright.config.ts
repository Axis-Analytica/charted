import { defineConfig } from "@playwright/test";

// The demo recording (bin/demo): the real UI and core with the real agent (Claude Code,
// signed in under the user's HOME), throwaway projects and state, and a video of the
// browser. Not part of the test suite: it costs a real agent request.
const tmp = process.env.CHARTED_DEMO_TMP ?? "/tmp/charted-demo";
const bin = `${process.env.CARGO_TARGET_DIR ?? `${process.cwd()}/../target`}/debug/charted-headless`;
const core = 7431;
const web = 1431;
// CHARTED_DEMO_FAKE=1 uses the tests' scripted stand-in, to try the recording for free.
const agent = process.env.CHARTED_DEMO_FAKE ? `CHARTED_AGENT_CMD="node ${process.cwd()}/e2e/fake-agent.mjs"` : "-u CHARTED_AGENT_CMD";

export default defineConfig({
  testDir: ".",
  testMatch: /\.demo\.ts$/,
  timeout: 600_000,
  workers: 1,
  reporter: [["list"]],
  outputDir: `${tmp}/results`,
  use: {
    baseURL: `http://localhost:${web}`,
    viewport: { width: 1600, height: 1000 },
    video: { mode: "on", size: { width: 1600, height: 1000 } },
    launchOptions: process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : undefined,
  },
  webServer: [
    {
      command: `rm -rf ${tmp}/docs ${tmp}/state && mkdir -p ${tmp}/docs ${tmp}/state && env ${agent} XDG_DOCUMENTS_DIR=${tmp}/docs ${bin} serve --port ${core} --state-dir ${tmp}/state`,
      port: core,
      cwd: "..",
      reuseExistingServer: false,
    },
    {
      command: `VITE_CHARTED_WS=ws://127.0.0.1:${core} npx vite --port ${web} --strictPort`,
      port: web,
      cwd: "..",
      reuseExistingServer: false,
    },
  ],
});
