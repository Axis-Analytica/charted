// Copy, cut and paste of visuals, between pages and between projects open in the same
// session. The clipboard holds JSON text with a marker (so a paste can tell it from
// any other text), and a copy is also kept in memory for when the system clipboard
// can't be read. Pasted visuals get fresh ids; fields the target model doesn't have
// are left out.
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { FilterKind } from "../bridge/generated/FilterKind";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Projection } from "../bridge/generated/Projection";
import type { ReportFilter } from "../bridge/generated/ReportFilter";
import type { Visual } from "../bridge/generated/Visual";

// Kept free of the app state (state/report pulls in the bridge) so it can be unit tested.
const fieldKey = (f: FieldRef) => (f.kind === "column" ? `c:${f.table}:${f.column}` : `m:${f.table}:${f.measure}`);
const newId = () => (typeof crypto.randomUUID === "function" ? crypto.randomUUID() : `${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`);

export const CLIPBOARD_MARKER = "application/x-charted-visuals";

export interface VisualClipboard {
  marker: typeof CLIPBOARD_MARKER;
  version: 1;
  /** Where the visuals came from (a paste there is offset). */
  project: string;
  page: string;
  visuals: Visual[];
  /** Each field's table and name, by field key, to find it in another model. */
  fields: Record<string, { table: string; name: string }>;
}

function fieldNames(model: Model, f: FieldRef): { table: string; name: string } | null {
  const t = model.tables.find((x) => x.id === f.table);
  const name = f.kind === "column" ? t?.columns.find((c) => c.id === f.column)?.name : t?.measures.find((m) => m.id === f.measure)?.name;
  return t && name ? { table: t.name, name } : null;
}

function projections(v: Visual): Projection[] {
  const out = Object.values(v.roles).flatMap((ps) => ps ?? []);
  const inFilter = (k: FilterKind | null | undefined) => (k && k.kind === "topN" ? [k.by] : []);
  for (const f of v.filters) out.push(f.field, ...inFilter(f.filter));
  out.push(...inFilter(v.selection));
  return out;
}

/** The clipboard contents for `visuals` on `page`. */
export function copyPayload(model: Model, project: string, page: Page, visuals: Visual[]): VisualClipboard {
  const fields: VisualClipboard["fields"] = {};
  for (const v of visuals)
    for (const p of projections(v)) {
      const n = fieldNames(model, p.field);
      if (n) fields[fieldKey(p.field)] = n;
    }
  return { marker: CLIPBOARD_MARKER, version: 1, project, page: page.id, visuals, fields };
}

export function parsePayload(text: string | null | undefined): VisualClipboard | null {
  if (!text || !text.includes(CLIPBOARD_MARKER)) return null;
  try {
    const p = JSON.parse(text) as VisualClipboard;
    return p.marker === CLIPBOARD_MARKER && Array.isArray(p.visuals) ? p : null;
  } catch {
    return null;
  }
}

/** The same field in `model`: by id when it is there under the same name, else by
 * table and field name. */
function resolver(model: Model, names: VisualClipboard["fields"]) {
  return (f: FieldRef): FieldRef | null => {
    const want = names[fieldKey(f)];
    const here = fieldNames(model, f);
    if (here && (!want || (here.table === want.table && here.name === want.name))) return f;
    if (!want) return null;
    const t = model.tables.find((x) => x.name === want.table);
    if (!t) return null;
    const c = t.columns.find((x) => x.name === want.name);
    if (c) return { kind: "column", table: t.id, column: c.id };
    const m = t.measures.find((x) => x.name === want.name);
    return m ? { kind: "measure", table: t.id, measure: m.id } : null;
  };
}

/** Visuals to add to `page` for a paste: fresh ids, fields mapped to `model` (those it
 * lacks dropped, counted in `dropped`), on top of the page's visuals, and offset when
 * they would land exactly on visuals already there (pasting where they were copied). */
export function pasteVisuals(model: Model, page: Page, clip: VisualClipboard): { visuals: Visual[]; dropped: number } {
  const resolve = resolver(model, clip.fields);
  let dropped = 0;
  const mapProjection = (p: Projection): Projection | null => {
    const field = resolve(p.field);
    if (!field) {
      dropped++;
      return null;
    }
    return { ...p, field };
  };
  const mapKind = (k: FilterKind | null | undefined): FilterKind | null => {
    if (!k) return null;
    if (k.kind !== "topN") return k;
    const by = mapProjection(k.by);
    return by ? { ...k, by } : null;
  };
  const top = page.visuals.reduce((m, v) => Math.max(m, v.position.z), 0);
  const order = [...clip.visuals].sort((a, b) => a.position.z - b.position.z);
  // Offset by 16 until no pasted visual sits exactly on one already there.
  const taken = new Set(page.visuals.map((v) => `${v.position.x},${v.position.y}`));
  let offset = 0;
  while (offset < 400 && order.some((v) => taken.has(`${Math.min(page.width - v.position.width, v.position.x + offset)},${Math.min(page.height - v.position.height, v.position.y + offset)}`))) offset += 16;
  const visuals = order.map((v, i): Visual => {
    const roles: Visual["roles"] = {};
    let changedRoles = false;
    for (const [role, ps] of Object.entries(v.roles)) {
      const kept = (ps ?? []).map(mapProjection).filter((p): p is Projection => !!p);
      if (kept.length !== (ps ?? []).length) changedRoles = true;
      if (kept.length) roles[role] = kept;
    }
    const filters = v.filters.flatMap((f): ReportFilter[] => {
      const field = mapProjection(f.field);
      if (!field) return [];
      const filter = f.filter.kind === "topN" ? mapKind(f.filter) : f.filter;
      return filter ? [{ id: newId(), field, filter }] : [];
    });
    const width = Math.min(v.position.width, page.width);
    const height = Math.min(v.position.height, page.height);
    return {
      ...v,
      id: newId(),
      roles,
      filters,
      // A sort or drill on fields that are gone no longer applies; a slicer without its field has no choice.
      sort: changedRoles ? null : v.sort,
      drill: changedRoles ? [] : v.drill,
      selection: changedRoles ? null : mapKind(v.selection),
      position: {
        x: Math.max(0, Math.min(page.width - width, v.position.x + offset)),
        y: Math.max(0, Math.min(page.height - height, v.position.y + offset)),
        width,
        height,
        z: top + 1 + i,
      },
    };
  });
  return { visuals, dropped };
}

let memory: string | null = null;

/** Puts visuals on the clipboard (and in memory). */
export async function writeClipboard(clip: VisualClipboard) {
  memory = JSON.stringify(clip);
  try {
    await navigator.clipboard?.writeText(memory);
  } catch {
    // The in-memory copy still pastes within the session.
  }
}

/** Visuals on the clipboard: the system clipboard's if it can be read, else the
 * session's last copy. Null when the clipboard holds something else. */
export async function readClipboard(): Promise<VisualClipboard | null> {
  try {
    const text = await navigator.clipboard?.readText();
    if (text) return parsePayload(text);
  } catch {
    // Not readable here: fall back to the session's copy.
  }
  return parsePayload(memory);
}
