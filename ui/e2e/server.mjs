// Starts the Rust core for the end-to-end tests (playwright.config.ts): a fresh
// throwaway folder whose home holds a copy of the desktop's Omarchy theme (so tests can
// switch it), documents and state folders, and the scripted stand-in agent. Node rather
// than shell, so it runs on Windows too.
import { spawn } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const { CHARTED_E2E_TMP: tmp, CHARTED_E2E_BIN: bin, CHARTED_E2E_PORT: port, CHARTED_E2E_AGENT: agent } = process.env;
const home = join(tmp, "home");
const theme = join(home, ".local/state/omarchy/current/theme");

rmSync(tmp, { recursive: true, force: true });
for (const d of [join(tmp, "docs"), join(tmp, "state"), join(tmp, "run"), theme]) mkdirSync(d, { recursive: true });
if (process.platform !== "win32") chmodSync(join(tmp, "run"), 0o700);
const current = join(homedir(), ".local/state/omarchy/current");
try {
  for (const f of readdirSync(join(current, "theme"))) copyFileSync(join(current, "theme", f), join(theme, f));
  copyFileSync(join(current, "theme.name"), join(theme, "../theme.name"));
} catch {
  // Not on Omarchy: the default theme.
}

const env = { ...process.env, HOME: home, USERPROFILE: home, XDG_DOCUMENTS_DIR: join(tmp, "docs"), XDG_RUNTIME_DIR: join(tmp, "run"), CHARTED_AGENT_CMD: `node ${agent}` };
// The desktop's Hyprland is hidden, so motion is on as on CI (Hyprland may turn it off).
delete env.HYPRLAND_INSTANCE_SIGNATURE;
const exe = process.platform === "win32" && !bin.endsWith(".exe") ? `${bin}.exe` : bin;
const child = spawn(exe, ["serve", "--port", port, "--state-dir", join(tmp, "state")], { env, stdio: "inherit" });
for (const sig of ["SIGINT", "SIGTERM"]) process.on(sig, () => child.kill(sig));
child.on("exit", (code) => process.exit(code ?? 1));
