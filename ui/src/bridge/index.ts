import { isTauri } from "@tauri-apps/api/core";
import { tauriTransport } from "./tauri";
import type { Transport } from "./transport";
import { wsTransport } from "./ws";

/** The transport for this environment: Tauri in the app, WebSocket in a browser. */
export const transport: Transport = isTauri() ? tauriTransport : wsTransport();

export * from "./transport";
