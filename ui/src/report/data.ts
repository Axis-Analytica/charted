// Loads a visual's data from the core: one request per visual, keyed by everything
// that changes its query, with a small cache so switching pages or undoing is instant.
import { tableFromIPC, type Vector } from "apache-arrow";
import { useEffect, useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";

export interface VisualData {
  rows: number;
  /** Columns by `role:index`. */
  columns: Map<string, Vector>;
  /** The total row (tables), by `role:index`. */
  totals?: Map<string, unknown>;
  ms: number;
}

const MAX_CACHED = 120;
const cache = new Map<string, Promise<VisualData>>();

/** Everything in the model that can change a visual's results. */
export function modelSignature(model: Model): string {
  return JSON.stringify([
    model.tables.map((t) => [t.id, t.source, t.columns.map((c) => [c.id, c.dataType, c.sortBy, c.kind]), t.measures.map((m) => [m.id, m.expression])]),
    model.relationships.map((r) => [r.id, r.fromColumn, r.toColumn, r.active, r.crossFilter, r.fromCardinality, r.toCardinality]),
  ]);
}

function decode(buf: ArrayBuffer): { rows: number; columns: Map<string, Vector> } {
  const table = tableFromIPC(new Uint8Array(buf));
  const columns = new Map<string, Vector>();
  table.schema.fields.forEach((f, i) => columns.set(f.name, table.getChildAt(i)!));
  return { rows: table.numRows, columns };
}

async function load(visual: Visual, totals: boolean): Promise<VisualData> {
  const t0 = performance.now();
  const main = transport.data({ type: "reportVisual", visual: visual.id, totals: false, filters: [] });
  const tot = totals ? transport.data({ type: "reportVisual", visual: visual.id, totals: true, filters: [] }) : null;
  const d = decode(await main);
  let totalsRow: Map<string, unknown> | undefined;
  if (tot) {
    const t = decode(await tot);
    totalsRow = new Map([...t.columns].map(([k, v]) => [k, v.get(0)]));
  }
  return { ...d, totals: totalsRow, ms: Math.round(performance.now() - t0) };
}

export function useVisualData(visual: Visual, page: Page, model: Model, signature: string, withTotals: boolean) {
  const hasFields = Object.values(visual.roles).some((r) => r && r.length > 0);
  const key = hasFields
    ? JSON.stringify([visual.id, visual.visualType, visual.roles, visual.filters, visual.sort, page.filters, model.report.filters, signature, withTotals])
    : "";
  const [state, setState] = useState<{ key: string; data: VisualData | null; error: string | null; loading: boolean }>({ key: "", data: null, error: null, loading: false });

  useEffect(() => {
    if (!key) {
      setState({ key, data: null, error: null, loading: false });
      return;
    }
    let live = true;
    let p = cache.get(key);
    if (!p) {
      p = load(visual, withTotals);
      cache.set(key, p);
      p.catch(() => cache.delete(key));
      while (cache.size > MAX_CACHED) cache.delete(cache.keys().next().value!);
    }
    // Keep showing the previous result while the next one loads.
    setState((s) => ({ ...s, key, loading: true }));
    p.then(
      (data) => live && setState({ key, data, error: null, loading: false }),
      (e) => live && setState({ key, data: null, error: errorMessage(e), loading: false }),
    );
    return () => {
      live = false;
    };
    // The key captures everything the query depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return state;
}
