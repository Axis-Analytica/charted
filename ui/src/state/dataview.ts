// Data view state: per-table sort, filters and column widths, plus the selected column
// and panel toggles. This is view state, so it never enters the undo history.
import { create } from "zustand";
import { transport } from "../bridge";
import type { ColumnFilter } from "../bridge/generated/ColumnFilter";
import type { DataRequest } from "../bridge/generated/DataRequest";
import type { Filter } from "../bridge/generated/Filter";
import type { SortKey } from "../bridge/generated/SortKey";
import type { ViewSpec } from "../bridge/generated/ViewSpec";

export interface TableView {
  sort: SortKey[];
  filters: ColumnFilter[];
  widths: Record<string, number>;
}

const emptyView: TableView = { sort: [], filters: [], widths: {} };

interface DataViewStore {
  views: Record<string, TableView>;
  /** Selected column per table. */
  selected: Record<string, string | undefined>;
  statsOpen: boolean;
  propsOpen: boolean;
  findOpen: boolean;
  /** Rows passing the filters and distinct values of the selected column, for the status bar. */
  counts: { table: string; rows: number | null; distinct: number | null } | null;
  /** Milliseconds from opening a view (or changing its sort/filters) to its first rows. */
  firstRows: number | null;
  /** A request from elsewhere (the Fields pane) to show a table, optionally with a new
   * calculated column's formula open. */
  goTo: { table: string; newColumn: boolean } | null;
  view(table: string): TableView;
  setSort(table: string, sort: SortKey[]): void;
  setFilter(table: string, column: string, filter: Filter | null): void;
  clearFilters(table: string): void;
  setWidth(table: string, column: string, width: number): void;
  select(table: string, column: string | undefined): void;
  reset(): void;
  set(patch: Partial<Pick<DataViewStore, "statsOpen" | "propsOpen" | "findOpen" | "counts" | "firstRows" | "goTo">>): void;
}

export const useDataView = create<DataViewStore>()((set, get) => ({
  views: {},
  selected: {},
  statsOpen: false,
  propsOpen: true,
  findOpen: false,
  counts: null,
  firstRows: null,
  goTo: null,
  view: (table) => get().views[table] ?? emptyView,
  setSort: (table, sort) => set((s) => ({ views: { ...s.views, [table]: { ...(s.views[table] ?? emptyView), sort } } })),
  setFilter: (table, column, filter) =>
    set((s) => {
      const v = s.views[table] ?? emptyView;
      const filters = v.filters.filter((f) => f.column !== column);
      if (filter) filters.push({ column, filter });
      return { views: { ...s.views, [table]: { ...v, filters } } };
    }),
  clearFilters: (table) => set((s) => ({ views: { ...s.views, [table]: { ...(s.views[table] ?? emptyView), filters: [] } } })),
  setWidth: (table, column, width) =>
    set((s) => {
      const v = s.views[table] ?? emptyView;
      return { views: { ...s.views, [table]: { ...v, widths: { ...v.widths, [column]: width } } } };
    }),
  select: (table, column) => set((s) => ({ selected: { ...s.selected, [table]: column } })),
  set: (patch) => set(patch),
  reset: () => set({ views: {}, selected: {}, counts: null, firstRows: null }),
}));

export function viewSpec(table: string, v: TableView): ViewSpec {
  return { table, sort: v.sort, filters: v.filters };
}

/** Sends a data request whose reply is JSON. */
export async function dataJson<T>(request: DataRequest): Promise<T> {
  const buf = await transport.data(request);
  return JSON.parse(new TextDecoder().decode(buf)) as T;
}

/** Cycles a column through ascending, descending and unsorted. With `add`, other sort
 * keys are kept (multi-column sort); otherwise the column becomes the only key. */
export function nextSort(sort: SortKey[], column: string, add: boolean): SortKey[] {
  const i = sort.findIndex((k) => k.column === column);
  const cur = sort[i];
  if (!add) {
    if (!cur || sort.length > 1) return [{ column, descending: false }];
    return cur.descending ? [] : [{ column, descending: true }];
  }
  if (!cur) return [...sort, { column, descending: false }];
  if (!cur.descending) return sort.map((k) => (k.column === column ? { column, descending: true } : k));
  return sort.filter((k) => k.column !== column);
}
