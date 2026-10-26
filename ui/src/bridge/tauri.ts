import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { EnvSnapshot } from "./generated/EnvSnapshot";
import type { PickOptions, Transport } from "./transport";

/** The desktop app also prints the report to PDF (WebKitGTK) and can exit with a code. */
export const tauriTransport: Transport & { printPdf(path: string, width: number, height: number): Promise<void>; exitWith(code: number): Promise<void> } = {
  rpc: (request) => invoke("rpc", { request }),
  data: (request) => invoke<ArrayBuffer>("data", { request }),
  env: () => invoke("env"),
  onEnv(callback) {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<EnvSnapshot>("env-changed", (e) => callback(e.payload)).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  },
  pick: (options: PickOptions) => invoke<string | null>("pick", { options }),
  initialPath: () => invoke<string | null>("initial_path"),
  quit: () => invoke("quit"),
  openIssue: (url) => invoke("open_issue", { url }),
  printPdf: (path, width, height) => invoke("print_pdf", { path, width, height }),
  exitWith: (code) => invoke("exit_with", { code }),
};
