// The query editor's draft (as Power Query's editor): edits apply to a copy of the
// project's queries, with their own undo, until Close & Apply loads them. The selected
// step is the number of steps applied in the preview (0 is the source).
import { create } from "zustand";
import type { Query } from "../bridge/generated/Query";
import type { Step } from "../bridge/generated/Step";

interface QueryDraft {
  /** The project's queries when the editor opened (to tell what changed). */
  base: Query[];
  queries: Query[];
  selected: string | null;
  /** Steps applied in the preview, by query id (absent: all). */
  upto: Record<string, number>;
  past: { label: string; queries: Query[] }[];
  future: { label: string; queries: Query[] }[];
  open(queries: Query[]): void;
  /** Replaces the draft (one undoable step); `select` picks the query and step shown. */
  change(label: string, queries: Query[], select?: { query: string; upto?: number }): void;
  select(id: string | null): void;
  selectStep(id: string, upto: number): void;
  undo(): void;
  redo(): void;
}

export const useQueryDraft = create<QueryDraft>()((set, get) => ({
  base: [],
  queries: [],
  selected: null,
  upto: {},
  past: [],
  future: [],
  open(queries) {
    const keep = get().selected;
    set({ base: queries, queries, selected: queries.some((q) => q.id === keep) ? keep : (queries[0]?.id ?? null), upto: {}, past: [], future: [] });
  },
  change(label, queries, select) {
    const s = get();
    const upto = { ...s.upto };
    if (select) {
      if (select.upto === undefined) delete upto[select.query];
      else upto[select.query] = select.upto;
    }
    set({ queries, past: [...s.past.slice(-199), { label, queries: s.queries }], future: [], selected: select?.query ?? s.selected, upto });
  },
  select(id) {
    set({ selected: id });
  },
  selectStep(id, upto) {
    set({ upto: { ...get().upto, [id]: upto } });
  },
  undo() {
    const s = get();
    const last = s.past.at(-1);
    if (!last) return;
    set({ queries: last.queries, past: s.past.slice(0, -1), future: [{ label: last.label, queries: s.queries }, ...s.future], upto: {} });
  },
  redo() {
    const s = get();
    const next = s.future[0];
    if (!next) return;
    set({ queries: next.queries, future: s.future.slice(1), past: [...s.past, { label: next.label, queries: s.queries }], upto: {} });
  },
}));

/** Whether the draft differs from what the project has. */
export function isDirty(base: Query[], queries: Query[]): boolean {
  return JSON.stringify(base) !== JSON.stringify(queries);
}

/** Steps applied in the preview for a query. */
export function uptoOf(q: Query, upto: Record<string, number>): number {
  return Math.min(upto[q.id] ?? q.steps.length, q.steps.length);
}

/** Inserts a step after the selected one (or at the end), as Power Query does. */
export function insertStep(q: Query, at: number, step: Step): Query {
  const steps = [...q.steps];
  steps.splice(at, 0, step);
  return { ...q, steps, stepNames: shiftNames(q.stepNames, at, 1) };
}

export function removeStep(q: Query, index: number): Query {
  const steps = q.steps.filter((_, i) => i !== index);
  const names: Record<string, string> = {};
  for (const [k, v] of Object.entries(q.stepNames ?? {})) {
    const i = Number(k);
    if (i < index) names[i] = v as string;
    else if (i > index) names[i - 1] = v as string;
  }
  return { ...q, steps, stepNames: names };
}

export function moveStep(q: Query, from: number, to: number): Query {
  const steps = [...q.steps];
  const [s] = steps.splice(from, 1);
  steps.splice(to, 0, s!);
  // Names move with their steps.
  const order = q.steps.map((_, i) => i);
  const [o] = order.splice(from, 1);
  order.splice(to, 0, o!);
  const names: Record<string, string> = {};
  order.forEach((old, i) => {
    const n = (q.stepNames ?? {})[old];
    if (n) names[i] = n as string;
  });
  return { ...q, steps, stepNames: names };
}

function shiftNames(names: Query["stepNames"], at: number, by: number): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(names ?? {})) {
    const i = Number(k);
    out[i >= at ? i + by : i] = v as string;
  }
  return out;
}

/** A table name for a new query, unique among the draft's. */
export function tableName(name: string, queries: Query[]): string {
  const base = name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "") || "query";
  let t = base;
  for (let i = 2; queries.some((q) => q.table === t); i++) t = `${base}_${i}`;
  return t;
}

export function uniqueQueryName(name: string, queries: Query[]): string {
  let n = name;
  for (let i = 2; queries.some((q) => q.name.toLowerCase() === n.toLowerCase()); i++) n = `${name} (${i})`;
  return n;
}
