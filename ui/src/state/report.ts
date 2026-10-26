// Report view state (not undoable): the current page, the selection, pane tabs, the
// formula bar, where the pointer last was on the canvas (so inserts land under it).
// Edits go through `commit` as model changes; the helpers here build those changes.
import { create } from "zustand";
import { hierarchy } from "./drill";
import type { Aggregation } from "../bridge/generated/Aggregation";
import type { Change } from "../bridge/generated/Change";
import type { Column } from "../bridge/generated/Column";
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { PageSettings } from "../bridge/generated/PageSettings";
import type { Projection } from "../bridge/generated/Projection";
import type { Visual } from "../bridge/generated/Visual";
import { msg } from "../i18n";
import { type RoleDef, visualDef } from "../report/registry";

export type View = "report" | "data" | "model" | "query";

interface ReportStore {
  view: View;
  page: string | null;
  selected: string[];
  /** Right-hand panes. */
  panes: { bookmarks: boolean; filters: boolean; visualizations: boolean; fields: boolean };
  vizTab: "build" | "format";
  /** Last pointer position on the page, in page units. */
  pointer: { x: number; y: number } | null;
  /** The measure being edited in the formula bar ("new:<tableId>" for a new one). */
  formula: string | null;
  /** The field selected in the Fields pane. */
  field: FieldRef | null;
  /** The quick measure dialog, open for a home table's id. */
  quick: string | null;
  /** Edit interactions mode: the selected visual's effect on the others is shown on each. */
  editInteractions: boolean;
  /** A visual shown on its own over the canvas (focus mode), optionally with its data. */
  focus: { visual: string; table: boolean } | null;
  set(patch: Partial<Omit<ReportStore, "set" | "select">>): void;
  select(ids: string[]): void;
}

export const useReport = create<ReportStore>()((set) => ({
  view: "report",
  page: null,
  selected: [],
  panes: { bookmarks: false, filters: true, visualizations: true, fields: true },
  vizTab: "build",
  pointer: null,
  formula: null,
  field: null,
  quick: null,
  editInteractions: false,
  focus: null,
  set: (patch) => set(patch),
  select: (ids) => set({ selected: ids }),
}));

let counter = 0;
/** A new id for pages, visuals and filters (UUID-shaped, as the model uses). */
export function newId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  counter += 1;
  return `${Date.now().toString(16)}-${counter.toString(16)}-${Math.random().toString(16).slice(2, 10)}`;
}

export const fieldKey = (f: FieldRef) => (f.kind === "column" ? `c:${f.table}:${f.column}` : `m:${f.table}:${f.measure}`);
export const sameField = (a: FieldRef, b: FieldRef) => fieldKey(a) === fieldKey(b);

export function columnOf(model: Model, f: FieldRef): Column | undefined {
  return f.kind === "column" ? model.tables.find((t) => t.id === f.table)?.columns.find((c) => c.id === f.column) : undefined;
}

export function fieldName(model: Model, f: FieldRef): string {
  const t = model.tables.find((x) => x.id === f.table);
  if (f.kind === "column") return t?.columns.find((c) => c.id === f.column)?.name ?? "(missing)";
  return t?.measures.find((m) => m.id === f.measure)?.name ?? "(missing)";
}

const AGG_LABEL: Record<Aggregation, string> = {
  sum: msg("Sum"),
  average: msg("Average"),
  min: msg("Min"),
  max: msg("Max"),
  count: msg("Count"),
  distinctCount: msg("Count (distinct)"),
};

export function aggregationLabel(a: Aggregation): string {
  return AGG_LABEL[a];
}

/** A projection's caption: "Sum of Quantity", the field name, or its display name. */
export function projectionName(model: Model, p: Projection, t: (s: string, ...a: (string | number)[]) => string = (s) => s): string {
  if (p.displayName) return p.displayName;
  const name = fieldName(model, p.field);
  return p.aggregation ? t("%1 of %2", t(AGG_LABEL[p.aggregation]), name) : name;
}

export const isGrouping = (p: Projection) => p.field.kind === "column" && !p.aggregation;

/** A visual's title: its own, or "Values by Groups" (matches the core's). */
export function visualTitle(model: Model, v: Visual, translate?: (s: string, ...a: (string | number)[]) => string): string {
  const own = (v.objects?.title as Record<string, unknown> | undefined)?.text;
  if (typeof own === "string" && own.trim()) return own;
  const t = translate ?? ((s: string, ...a: (string | number)[]) => s.replace(/%(\d)/g, (_, i) => String(a[Number(i) - 1])));
  const groups: string[] = [];
  const values: string[] = [];
  // A hierarchy is named by the levels shown.
  const h = hierarchy(v);
  for (const [role, ps] of Object.entries(v.roles))
    (ps ?? []).forEach((p, i) => {
      if (h && role === h.role && !h.shown.includes(i)) return;
      (isGrouping(p) ? groups : values).push(projectionName(model, p, t));
    });
  if (values.length && groups.length) return t("%1 by %2", values.join(t(" and ")), groups.join(t(" and ")));
  return (values.length ? values : groups).join(t(" and "));
}

/** The default summarisation of a column dropped as a value. */
export function defaultAggregation(c: Column): Aggregation {
  switch (c.summarizeBy) {
    case "Sum": return "sum";
    case "Average": return "average";
    case "Min": return "min";
    case "Max": return "max";
    case "Count": return "count";
    case "DistinctCount": return "distinctCount";
    case "None":
    case "Default":
      return c.dataType === "Int64" || c.dataType === "Double" || c.dataType === "Decimal" ? "sum" : "count";
  }
}

/** The projection a field makes in a role: values summarise columns, groupings don't. */
export function projectionFor(model: Model, field: FieldRef, role: RoleDef): Projection {
  const col = columnOf(model, field);
  const numeric = col && (col.dataType === "Int64" || col.dataType === "Double" || col.dataType === "Decimal");
  const summarise = col && (role.kind === "value" || (role.kind === "any" && numeric && col.summarizeBy !== "None" && !col.isKey && !col.sortBy));
  return { field, aggregation: summarise && col ? defaultAggregation(col) : null, displayName: null };
}

/** The role a field goes to when added without a target (checkbox, Enter, drop on the visual). */
export function roleFor(model: Model, v: Visual, field: FieldRef): RoleDef | undefined {
  const def = visualDef(v.visualType);
  if (!def) return undefined;
  const limit = (r: RoleDef) => r.autoMax ?? r.max;
  const free = (r: RoleDef) => limit(r) === undefined || (v.roles[r.name]?.length ?? 0) < limit(r)!;
  const isMeasure = field.kind === "measure";
  const col = columnOf(model, field);
  const numeric = col && (col.dataType === "Int64" || col.dataType === "Double" || col.dataType === "Decimal") && col.summarizeBy !== "None" && !col.isKey;
  const prefer = isMeasure || numeric ? ["value", "any"] : ["grouping", "any"];
  for (const kind of prefer) {
    const r = def.roles.find((x) => x.kind === kind && free(x));
    if (r) return r;
  }
  return def.roles.find((r) => free(r) && (r.kind === "any" || r.kind === (isMeasure ? "value" : "grouping")));
}

export function withField(model: Model, v: Visual, field: FieldRef, roleName?: string, index?: number): Visual | null {
  const def = visualDef(v.visualType);
  const role = roleName ? def?.roles.find((r) => r.name === roleName) : roleFor(model, v, field);
  if (!role) return null;
  if (field.kind === "measure" && role.kind === "grouping") return null;
  const list = [...(v.roles[role.name] ?? [])];
  const p = projectionFor(model, field, role);
  if (role.max !== undefined && list.length >= role.max) list.splice(role.max - 1, list.length);
  list.splice(index ?? list.length, 0, p);
  // A slicer's choice belongs to its field.
  return { ...v, roles: { ...v.roles, [role.name]: list }, selection: v.visualType === "slicer" ? null : v.selection };
}

export function withoutField(v: Visual, role: string, index: number): Visual {
  const list = [...(v.roles[role] ?? [])];
  list.splice(index, 1);
  const roles = { ...v.roles, [role]: list };
  if (!list.length) delete roles[role];
  // A sort on a removed field no longer applies.
  const sort = v.sort && v.sort.role === role && v.sort.index >= index ? null : v.sort;
  return { ...v, roles, sort, selection: v.visualType === "slicer" ? null : v.selection };
}

/** Change(s) for an updated visual. */
export const updateVisual = (page: string, visual: Visual): Change => ({ op: "updateVisual", page, visual });

/** A new, empty visual of `type` centred on `at` (or in the next free spot), clamped to the page. */
export function newVisual(page: Page, type: string, at: { x: number; y: number } | null): Visual {
  const def = visualDef(type);
  const w = def?.size.width ?? 400;
  const h = def?.size.height ?? 260;
  let x: number;
  let y: number;
  if (at) {
    x = at.x - w / 2;
    y = at.y - h / 2;
  } else {
    // Cascade from the top left so new visuals don't stack exactly.
    const n = page.visuals.length;
    x = 24 + (n % 6) * 24;
    y = 24 + (n % 6) * 24;
  }
  const snap = (n: number) => Math.round(n / 8) * 8;
  x = snap(Math.max(0, Math.min(page.width - w, x)));
  y = snap(Math.max(0, Math.min(page.height - h, y)));
  const z = page.visuals.reduce((m, v) => Math.max(m, v.position.z), 0) + 1;
  return { id: newId(), visualType: type, position: { x, y, width: w, height: h, z }, roles: {}, filters: [], objects: {} };
}

/** A page's settings (everything but its id and visuals), for `setPage`. */
export function pageSettings(p: Page): PageSettings {
  const { id: _id, visuals: _visuals, ...settings } = p;
  return settings;
}

/** The page shown: the chosen one if it still exists, else the first. */
export function currentPage(model: Model, id: string | null): Page | null {
  return model.report.pages.find((p) => p.id === id) ?? model.report.pages[0] ?? null;
}

/** A visual in the latest model (for `edit`). */
export function visualIn(model: Model, page: string, id: string): Visual | undefined {
  return model.report.pages.find((p) => p.id === page)?.visuals.find((v) => v.id === id);
}

/** Replaces a visual using its latest state: `edit(label, editVisual(page, id, fn))`. */
export function editVisual(page: string, id: string, fn: (v: Visual, model: Model) => Visual | null) {
  return (model: Model): Change[] | null => {
    const v = visualIn(model, page, id);
    const next = v && fn(v, model);
    return next ? [updateVisual(page, next)] : null;
  };
}
