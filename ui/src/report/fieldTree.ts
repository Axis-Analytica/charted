// Pure helpers for the Fields pane: display folders, hierarchies, unique names and
// moving a measure to another table (which rewrites the report's references to it).
import type { Change } from "../bridge/generated/Change";
import type { Column } from "../bridge/generated/Column";
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { Hierarchy } from "../bridge/generated/Hierarchy";
import type { Model } from "../bridge/generated/Model";
import type { Table } from "../bridge/generated/Table";

/** A display folder's path: Power BI nests folders with `\`. */
export function folderPath(folder: string | null | undefined): string[] {
  return (folder ?? "")
    .split("\\")
    .map((s) => s.trim())
    .filter(Boolean);
}

/** A folder path written back as Power BI stores it, or null for the table's root. */
export const folderString = (path: string[]): string | null => (path.length ? path.join("\\") : null);

export interface FolderNode<T> {
  name: string;
  /** The folder's full path, `\`-separated ("" for the root). */
  path: string;
  folders: FolderNode<T>[];
  items: T[];
}

/** Items grouped into nested folders: subfolders sorted by name, items in the given order. */
export function folderTree<T>(items: T[], folderOf: (item: T) => string | null | undefined): FolderNode<T> {
  const root: FolderNode<T> = { name: "", path: "", folders: [], items: [] };
  for (const item of items) {
    let node = root;
    for (const seg of folderPath(folderOf(item))) {
      let next = node.folders.find((f) => f.name.toLowerCase() === seg.toLowerCase());
      if (!next) {
        next = { name: seg, path: node.path ? `${node.path}\\${seg}` : seg, folders: [], items: [] };
        node.folders.push(next);
      }
      node = next;
    }
    node.items.push(item);
  }
  const sort = (n: FolderNode<T>) => {
    n.folders.sort((a, b) => a.name.localeCompare(b.name));
    n.folders.forEach(sort);
  };
  sort(root);
  return root;
}

/** Every folder path used in a table, prefixes included, sorted (for suggestions). */
export function tableFolders(table: Table): string[] {
  const all = new Set<string>();
  for (const f of [...table.measures, ...table.columns, ...(table.hierarchies ?? [])].map((x) => x.displayFolder)) {
    const path = folderPath(f);
    path.forEach((_, i) => all.add(path.slice(0, i + 1).join("\\")));
  }
  return [...all].sort((a, b) => a.localeCompare(b));
}

/** True when `name` is free among a table's columns, measures and hierarchies. */
export function nameFree(table: Table, name: string, except?: string): boolean {
  const n = name.trim().toLowerCase();
  return ![...table.columns, ...table.measures, ...(table.hierarchies ?? [])].some((x) => x.id !== except && x.name.toLowerCase() === n);
}

/** `base`, or `base 2`, `base 3`… whichever is free in the table. */
export function uniqueFieldName(table: Table, base: string): string {
  if (nameFree(table, base)) return base;
  for (let i = 2; ; i++) if (nameFree(table, `${base} ${i}`)) return `${base} ${i}`;
}

/** The fields a hierarchy adds to a visual: its levels' columns, top first. */
export function hierarchyFields(table: Table, h: Hierarchy): FieldRef[] {
  return h.levels.filter((l) => table.columns.some((c) => c.id === l.column)).map((l) => ({ kind: "column", table: table.id, column: l.column }));
}

/** A new hierarchy with one level, named after its column as Power BI does. */
export function newHierarchy(table: Table, column: Column, id: string): Hierarchy {
  return { id, name: uniqueFieldName(table, `${column.name} Hierarchy`), description: null, levels: [{ name: column.name, column: column.id }], hidden: false, displayFolder: null };
}

/** The hierarchy with `column` added as its lowest level (under a free level name). */
export function withLevel(h: Hierarchy, column: Column): Hierarchy {
  if (h.levels.some((l) => l.column === column.id)) return h;
  let name = column.name;
  for (let i = 2; h.levels.some((l) => l.name.toLowerCase() === name.toLowerCase()); i++) name = `${column.name} ${i}`;
  return { ...h, levels: [...h.levels, { name, column: column.id }] };
}

/** `value` with every reference to the measure moved from one table to another; the same
 * object when nothing refers to it. */
function retarget<T>(value: T, measure: string, from: string, to: string): T {
  if (Array.isArray(value)) {
    let changed = false;
    const out = value.map((x) => {
      const y = retarget(x, measure, from, to);
      if (y !== x) changed = true;
      return y;
    });
    return (changed ? out : value) as T;
  }
  if (value && typeof value === "object") {
    const o = value as Record<string, unknown>;
    if (o.kind === "measure" && o.measure === measure && o.table === from) return { ...o, table: to } as T;
    let out: Record<string, unknown> | null = null;
    for (const [k, v] of Object.entries(o)) {
      const y = retarget(v, measure, from, to);
      if (y !== v) (out ??= { ...o })[k] = y;
    }
    return (out ?? value) as T;
  }
  return value;
}

/** Changes that move a measure to another table: the measure itself, then every visual,
 * page, report filter and bookmark that refers to it. */
export function moveMeasure(model: Model, measure: string, from: string, to: string, edited?: (m: Table["measures"][number]) => Table["measures"][number]): Change[] {
  const m = model.tables.find((t) => t.id === from)?.measures.find((x) => x.id === measure);
  if (!m || from === to) return [];
  const changes: Change[] = [
    { op: "removeMeasure", table: from, measure },
    { op: "addMeasure", table: to, measure: edited ? edited(m) : m, index: null },
  ];
  for (const p of model.report.pages) {
    for (const v of p.visuals) {
      const next = retarget(v, measure, from, to);
      if (next !== v) changes.push({ op: "updateVisual", page: p.id, visual: next });
    }
    const { id: _id, visuals: _visuals, ...settings } = p;
    const next = retarget(settings, measure, from, to);
    if (next !== settings) changes.push({ op: "setPage", page: p.id, settings: next });
  }
  const filters = retarget(model.report.filters, measure, from, to);
  if (filters !== model.report.filters) changes.push({ op: "setReportFilters", filters });
  const bookmarks = model.report.bookmarks ?? [];
  const nextBookmarks = retarget(bookmarks, measure, from, to);
  if (nextBookmarks !== bookmarks) changes.push({ op: "setBookmarks", bookmarks: nextBookmarks });
  return changes;
}
