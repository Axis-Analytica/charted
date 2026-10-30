// Runs the DAX language service (Rust compiled to WebAssembly) off the main thread.
import init, * as dax from "./pkg/charted_wasm.js";

type Method = "setModel" | "diagnostics" | "table_diagnostics" | "item_diagnostics" | "complete" | "signature" | "hover" | "format" | "functions";
type Call = { id: number; method: Method; args: unknown[] };

const ready = init();

self.onmessage = async (e: MessageEvent<Call>) => {
  const { id, method, args } = e.data;
  try {
    await ready;
    const fn = dax[method] as (...a: unknown[]) => unknown;
    const out = fn(...args);
    // Everything but setModel and format answers JSON.
    const result = typeof out === "string" && method !== "format" ? JSON.parse(out) : out;
    (self as unknown as Worker).postMessage({ id, result });
  } catch (err) {
    (self as unknown as Worker).postMessage({ id, error: String(err) });
  }
};
