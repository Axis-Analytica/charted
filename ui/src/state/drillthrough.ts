// Drill through: a data point's values carried to a page set up for them (its
// drill-through fields), which then shows that point's data. The filters are view
// state: they last until the user goes back or leaves the page.
import { create } from "zustand";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Projection } from "../bridge/generated/Projection";
import type { QueryFilter } from "../bridge/generated/QueryFilter";
import type { Visual } from "../bridge/generated/Visual";
import { sameField, useReport } from "./report";

export interface DrillThrough {
  /** The page drilled to, and the page to go back to. */
  target: string;
  from: string;
  filters: QueryFilter[];
  /** What the point was: each drill-through field and its value. */
  point: { field: Projection; value: string | null }[];
}

export const useDrillThrough = create<{ current: DrillThrough | null; set(d: DrillThrough | null): void }>((set) => ({
  current: null,
  set: (current) => set({ current }),
}));

/** Pages a point with these fields can drill through to: every drill-through field of
 * the page is among them. */
export function drillTargets(model: Model, from: Page, fields: Projection[]): Page[] {
  return model.report.pages.filter((p) => p.id !== from.id && (p.drillthrough ?? []).length > 0 && (p.drillthrough ?? []).every((d) => fields.some((f) => sameField(f.field, d.field))));
}

const equals = (field: Projection, value: string | null): QueryFilter => ({ field: { ...field, aggregation: null }, filter: { kind: "basic", values: [value], exclude: false } });

/** Goes to `target` showing the point (its fields' values). With the target's "Keep all
 * filters", the source visual's page filters, slicers and own filters come too, and
 * the point's other fields. */
export function drillThrough(from: Page, visual: Visual, fields: Projection[], tuple: (string | null)[], target: Page) {
  const keepAll = target.keepAllFilters ?? true;
  const point = (target.drillthrough ?? []).map((d) => {
    const i = fields.findIndex((f) => sameField(f.field, d.field));
    return { field: d, value: tuple[i] ?? null };
  });
  const filters: QueryFilter[] = point.map((p) => equals(p.field, p.value));
  if (keepAll) {
    fields.forEach((f, i) => {
      if (!point.some((p) => sameField(p.field.field, f.field))) filters.push(equals(f, tuple[i] ?? null));
    });
    for (const f of [...from.filters, ...visual.filters]) if (f.filter.kind !== "all") filters.push({ field: f.field, filter: f.filter });
    for (const s of from.visuals) {
      const field = s.visualType === "slicer" ? s.roles.field?.[0] : undefined;
      if (field && s.selection && s.selection.kind !== "all") filters.push({ field: { ...field, aggregation: null }, filter: s.selection });
    }
  }
  useDrillThrough.getState().set({ target: target.id, from: from.id, filters, point });
  useReport.getState().set({ page: target.id, selected: [] });
}

/** Back to the page drilled from. */
export function drillBack() {
  const d = useDrillThrough.getState().current;
  useDrillThrough.getState().set(null);
  if (d) useReport.getState().set({ page: d.from, selected: [] });
}

const NONE: QueryFilter[] = [];

/** The drill-through filters on a page (none unless it is the one drilled to). */
export function drillFilters(d: DrillThrough | null, page: string): QueryFilter[] {
  return d && d.target === page ? d.filters : NONE;
}
