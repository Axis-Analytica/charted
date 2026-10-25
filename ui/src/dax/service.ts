// The main-thread side of the DAX language service: one worker, shared by every
// editor, kept in step with the open model.
import type { Model } from "../bridge/generated/Model";
import DaxWorker from "./dax.worker.ts?worker";

export interface DaxDiagnostic {
  message: string;
  start: number;
  end: number;
}
export interface DaxRange {
  start: number;
  end: number;
}
export interface DaxCompletion {
  label: string;
  kind: "function" | "table" | "column" | "measure" | "variable" | "keyword";
  detail: string;
  documentation: string | null;
  insertText: string;
  replace: DaxRange;
  sort: string;
}
export interface DaxSignature {
  label: string;
  documentation: string;
  params: string[];
  active: number;
}
export interface DaxHover {
  contents: string;
  range: DaxRange;
}

let worker: Worker | null = null;
let next = 0;
const pending = new Map<number, { resolve(v: unknown): void; reject(e: Error): void }>();
let sentModel: Model | null = null;

function call<T>(method: string, ...args: unknown[]): Promise<T> {
  if (!worker) {
    worker = new DaxWorker();
    worker.onmessage = (e: MessageEvent<{ id: number; result?: unknown; error?: string }>) => {
      const p = pending.get(e.data.id);
      pending.delete(e.data.id);
      if (e.data.error !== undefined) p?.reject(new Error(e.data.error));
      else p?.resolve(e.data.result);
    };
  }
  const id = next++;
  return new Promise<T>((resolve, reject) => {
    pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
    worker!.postMessage({ id, method, args });
  });
}

/** Makes sure the service resolves names against `model` (sent only when it changes). */
export function syncModel(model: Model) {
  if (model === sentModel) return;
  sentModel = model;
  // The report is irrelevant to formulas; leave it out of the copy.
  void call("setModel", JSON.stringify({ ...model, report: { pages: [], filters: [], diagram: {} } }));
}

export const dax = {
  diagnostics: (text: string, home: string | null) => call<DaxDiagnostic[]>("diagnostics", text, home ?? undefined),
  complete: (text: string, offset: number, home: string | null) => call<DaxCompletion[]>("complete", text, offset, home ?? undefined),
  signature: (text: string, offset: number) => call<DaxSignature | null>("signature", text, offset),
  hover: (text: string, offset: number, home: string | null) => call<DaxHover | null>("hover", text, offset, home ?? undefined),
  format: (text: string) => call<string>("format", text),
};

/** Splits `Name = expression`. Only a plain name before the first `=` counts, so
 * `IF ( a = b, … )` is all expression. */
export function splitFormula(text: string): { name: string | null; exprStart: number } {
  const m = /^\s*([^=()[\]'"{}]*?)\s*=/.exec(text);
  if (!m) return { name: null, exprStart: 0 };
  const name = (m[1] ?? "").trim();
  // `= expression`: no name, and the `=` is not part of the expression.
  return { name: name || null, exprStart: m[0].length };
}
