// The data selection on the report page (not undoable, not saved): the points chosen
// in one visual, as tuples of their group values. Other visuals on the page are
// filtered or highlighted by it, as the page's interactions say. Ctrl-click adds to
// (or removes from) the selection in the same visual; a plain click replaces it, and
// clicking the only selected point again clears it.
import { create } from "zustand";
import type { InteractionKind } from "../bridge/generated/InteractionKind";
import type { Page } from "../bridge/generated/Page";
import type { Projection } from "../bridge/generated/Projection";
import type { QueryFilter } from "../bridge/generated/QueryFilter";
import type { Visual } from "../bridge/generated/Visual";

export interface DataSelection {
  page: string;
  /** The visual the points were chosen in. */
  source: string;
  /** The fields each tuple gives a value for. */
  fields: Projection[];
  /** Selected points: group values as text (as filters compare them), null for blank. */
  tuples: (string | null)[][];
}

interface SelectionStore {
  selection: DataSelection | null;
  choose(page: string, source: string, fields: Projection[], tuple: (string | null)[], additive: boolean): void;
  clear(): void;
}

const same = (a: (string | null)[], b: (string | null)[]) => a.length === b.length && a.every((x, i) => x === b[i]);

export const useSelection = create<SelectionStore>()((set, get) => ({
  selection: null,
  choose(page, source, fields, tuple, additive) {
    const cur = get().selection;
    const sameScope = cur && cur.page === page && cur.source === source && JSON.stringify(cur.fields) === JSON.stringify(fields);
    if (sameScope && additive) {
      const has = cur.tuples.some((t) => same(t, tuple));
      const tuples = has ? cur.tuples.filter((t) => !same(t, tuple)) : [...cur.tuples, tuple];
      set({ selection: tuples.length ? { ...cur, tuples } : null });
      return;
    }
    if (sameScope && cur.tuples.length === 1 && same(cur.tuples[0]!, tuple)) {
      set({ selection: null });
      return;
    }
    set({ selection: { page, source, fields, tuples: [tuple] } });
  },
  clear() {
    if (get().selection) set({ selection: null });
  },
}));

/** Whether a visual type can show a selection elsewhere as a highlight (matches the core). */
export function canHighlight(type: string): boolean {
  return ["columnChart", "barChart", "stackedColumnChart", "stackedBarChart", "pieChart", "donutChart", "scatterChart"].includes(type);
}

/** Visuals that show data and so take part in interactions. */
export function isDataVisual(type: string): boolean {
  return !["textbox", "image", "shape"].includes(type);
}

/** What a selection in `source` does to `target` (matches `Page::interaction` in the core). */
export function interaction(page: Page, source: string, target: string): InteractionKind {
  const set = page.interactions?.find((i) => i.source === source && i.target === target);
  if (set) return set.kind;
  const v = page.visuals.find((x) => x.id === target);
  return v && canHighlight(v.visualType) ? "highlight" : "filter";
}

/** The kinds a target can be set to. */
export function interactionKinds(target: Visual): InteractionKind[] {
  return canHighlight(target.visualType) ? ["filter", "highlight", "none"] : ["filter", "none"];
}

/** A selection as filters: each field keeps the values chosen for it. (Several points
 * chosen across two fields keep every combination of their values.) */
export function selectionFilters(s: DataSelection): QueryFilter[] {
  return s.fields.map((field, i) => {
    const values: (string | null)[] = [];
    for (const t of s.tuples) if (!values.some((v) => v === t[i])) values.push(t[i] ?? null);
    return { field: { ...field, aggregation: null }, filter: { kind: "basic", values, exclude: false } };
  });
}

/** How `visual` is affected by the current selection: filters to add, filters that
 * highlight, or nothing. */
export function selectionEffect(s: DataSelection | null, page: Page, visual: Visual): { filters: QueryFilter[]; highlight: QueryFilter[] } {
  const none = { filters: [], highlight: [] };
  if (!s || s.page !== page.id || s.source === visual.id || !isDataVisual(visual.visualType)) return none;
  if (!page.visuals.some((v) => v.id === s.source)) return none;
  const kind = interaction(page, s.source, visual.id);
  if (kind === "none") return none;
  const f = selectionFilters(s);
  return kind === "highlight" && canHighlight(visual.visualType) ? { filters: [], highlight: f } : { filters: f, highlight: [] };
}

/** Whether a tuple of `visual`'s own points is selected (for dimming the rest). */
export function isChosen(s: DataSelection | null, visual: string, tuple: (string | null)[]): boolean | null {
  if (!s || s.source !== visual) return null;
  return s.tuples.some((t) => same(t, tuple));
}
