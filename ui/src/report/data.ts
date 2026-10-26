// Loads a visual's data from the core: one request per visual, keyed by everything
// that changes its query (including the page's slicers and the selection in another
// visual), with a small cache so switching pages, undoing or clearing a selection is
// instant. A newer request for the same visual cancels the one still running.
import { tableFromIPC, type Vector } from "apache-arrow";
import { createContext, useContext, useEffect, useState } from "react";
import { errorMessage, isApiError, transport } from "../bridge";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { QueryFilter } from "../bridge/generated/QueryFilter";
import type { Visual } from "../bridge/generated/Visual";
import { drillFilters, useDrillThrough } from "../state/drillthrough";
import { selectionEffect, useSelection } from "../state/selection";
import { useSlicerSearch } from "../state/slicersearch";
import { useApp } from "../state/app";
import { useViewAs, viewedRoles } from "../state/viewas";

export interface VisualData {
  rows: number;
  /** Columns by `role:index` (also `key:role:index`, `hl:role:index`, and a matrix's
   * `level:rows`, `level:columns` and `order`). */
  columns: Map<string, Vector>;
  /** The total row (tables), by `role:index`. */
  totals?: Map<string, unknown>;
  /** Whether the values come with highlighted parts (`hl:…`). */
  highlighted: boolean;
  ms: number;
}

/** Filters every visual under it adds to its query (a tooltip page's point). */
export const ExtraFilters = createContext<QueryFilter[]>([]);

const MAX_CACHED = 160;
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

async function load(visual: Visual, totals: boolean, filters: QueryFilter[], highlight: QueryFilter[], search: string, roles: string[]): Promise<VisualData> {
  const t0 = performance.now();
  const main = transport.data({ type: "reportVisual", visual: visual.id, totals: false, filters, highlight, tag: visual.id, ...(search ? { search } : {}), roles });
  const tot = totals ? transport.data({ type: "reportVisual", visual: visual.id, totals: true, filters, highlight: [], tag: `${visual.id}:totals`, roles }) : null;
  const d = decode(await main);
  let totalsRow: Map<string, unknown> | undefined;
  if (tot) {
    const t = decode(await tot);
    totalsRow = new Map([...t.columns].map(([k, v]) => [k, v.get(0)]));
  }
  return { ...d, totals: totalsRow, highlighted: highlight.length > 0, ms: Math.round(performance.now() - t0) };
}

/** The page's state that every visual's query depends on: its slicers' choices and
 * interactions. */
function pageSignature(page: Page): string {
  return JSON.stringify([page.filters, page.interactions ?? [], page.visuals.filter((v) => v.visualType === "slicer").map((s) => [s.id, s.roles.field, s.selection ?? null])]);
}

export function useVisualData(visual: Visual, page: Page, model: Model, signature: string, withTotals: boolean) {
  const hasFields = Object.values(visual.roles).some((r) => r && r.length > 0);
  const selection = useSelection((s) => s.selection);
  const drilled = useDrillThrough((s) => drillFilters(s.current, page.id));
  const extra = useContext(ExtraFilters);
  const own = selectionEffect(selection, page, visual);
  const effect = drilled.length || extra.length ? { ...own, filters: [...drilled, ...extra, ...own.filters] } : own;
  const search = useSlicerSearch((s) => (visual.visualType === "slicer" ? (s.byVisual[visual.id] ?? "") : ""));
  const roles = useViewedRoles();
  // The viewed roles' filters change what every query returns.
  const security = roles.length ? model.roles?.filter((r) => roles.includes(r.id)).map((r) => [r.id, r.filters]) : null;
  const key = hasFields
    ? JSON.stringify([visual.id, visual.visualType, visual.roles, visual.filters, visual.sort, visual.drill ?? [], visual.visualType === "slicer" ? visual.objects?.slicer?.style : null, formattingFields(visual), pageSignature(page), model.report.filters, signature, withTotals, effect, search, security])
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
      p = load(visual, withTotals, effect.filters, effect.highlight, search, roles);
      cache.set(key, p);
      p.catch(() => cache.delete(key));
      while (cache.size > MAX_CACHED) cache.delete(cache.keys().next().value!);
    }
    // Keep showing the previous result while the next one loads.
    setState((s) => ({ ...s, key, loading: true }));
    p.then(
      (data) => live && setState({ key, data, error: null, loading: false }),
      (e) => {
        // A newer request replaced this one; its result will arrive instead.
        if (isApiError(e) && e.code === "cancelled") return;
        if (live) setState({ key, data: null, error: errorMessage(e), loading: false });
      },
    );
    return () => {
      live = false;
    };
    // The key captures everything the query depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return state;
}

/** Ids of the roles the open project's report is viewed as (empty when not viewing). */
export function useViewedRoles(): string[] {
  const project = useApp((s) => s.state?.project?.path);
  return useViewAs((s) => viewedRoles(s, project));
}

/** The fields conditional formatting reads (they are hidden values of the query). */
function formattingFields(visual: Visual): unknown {
  if (visual.visualType !== "table" && visual.visualType !== "matrix") return null;
  const cf = visual.objects?.conditionalFormatting as Record<string, Record<string, { field?: unknown }>> | undefined;
  if (!cf) return null;
  return Object.entries(cf).flatMap(([col, kinds]) => Object.entries(kinds ?? {}).map(([k, r]) => [col, k, r?.field ?? null]));
}

/** Text keys and values of a column, as arrays (null for blank). */
export function keysOf(data: VisualData, name: string): (string | null)[] {
  const v = data.columns.get(`key:${name}`);
  const out: (string | null)[] = [];
  for (let r = 0; r < data.rows; r++) {
    const x = v?.get(r);
    out.push(x === null || x === undefined ? null : String(x));
  }
  return out;
}
