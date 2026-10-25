// WebSocket transport for running the UI in a browser against `charted serve --dev`
// (used by end-to-end tests and screenshots). Text frames carry JSON; data replies are
// binary frames: an 8-byte little-endian request id followed by Arrow IPC bytes.
import type { EnvSnapshot } from "./generated/EnvSnapshot";
import type { PickOptions, Transport } from "./transport";

interface Pending {
  resolve(v: unknown): void;
  reject(e: unknown): void;
}

export function wsTransport(): Transport {
  const params = new URLSearchParams(location.search);
  const url = params.get("ws") ?? import.meta.env.VITE_CHARTED_WS ?? "ws://127.0.0.1:7420";
  const token = params.get("token") ?? import.meta.env.VITE_CHARTED_TOKEN ?? "";
  let nextId = 1;
  const pending = new Map<number, Pending>();
  const envListeners = new Set<(e: EnvSnapshot) => void>();
  let socket: Promise<WebSocket> | null = null;

  const connect = () => {
    socket ??= new Promise<WebSocket>((resolve, reject) => {
      const ws = new WebSocket(`${url}/?token=${encodeURIComponent(token)}`);
      ws.binaryType = "arraybuffer";
      ws.onopen = () => resolve(ws);
      ws.onerror = () => reject({ code: "transport", message: `cannot reach the charted dev server at ${url}` });
      ws.onclose = () => {
        socket = null;
        for (const p of pending.values()) p.reject({ code: "transport", message: "connection to charted closed" });
        pending.clear();
      };
      ws.onmessage = (ev) => {
        if (ev.data instanceof ArrayBuffer) {
          const id = Number(new DataView(ev.data).getBigUint64(0, true));
          pending.get(id)?.resolve(ev.data.slice(8));
          pending.delete(id);
          return;
        }
        const msg = JSON.parse(ev.data as string);
        if (msg.event === "env") {
          for (const l of envListeners) l(msg.payload);
          return;
        }
        const p = pending.get(msg.id);
        if (!p) return;
        pending.delete(msg.id);
        if ("err" in msg) p.reject(msg.err);
        else p.resolve(msg.ok);
      };
    });
    return socket;
  };

  const call = async <T>(method: string, params?: unknown): Promise<T> => {
    const ws = await connect();
    const id = nextId++;
    return new Promise<T>((resolve, reject) => {
      pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });
  };

  return {
    rpc: (request) => call("rpc", request),
    data: (request) => call("data", request),
    env: () => call("env"),
    onEnv(cb) {
      envListeners.add(cb);
      void connect();
      return () => envListeners.delete(cb);
    },
    // End-to-end tests answer file dialogs by setting window.__chartedPick.
    pick: (options) => {
      const fake = (window as { __chartedPick?: (o: PickOptions) => string | null }).__chartedPick;
      return fake ? Promise.resolve(fake(options)) : call("pick", options);
    },
    initialPath: async () => params.get("open"),
    quit: () => call("quit"),
  };
}
