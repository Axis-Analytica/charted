// The matrix: a row hierarchy (stepped, with subtotals) against the members of a column
// hierarchy, with totals. The core sends every level at once (`level:rows`,
// `level:columns`), so expanding and collapsing is instant. Rows start collapsed to the
// first level, as in Power BI; Expand all and Collapse all are in the visual's menu.
import { ChevronDown, ChevronRight } from "lucide-react";
import { useMemo } from "react";
import { create } from "zustand";
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { projectionName } from "../../state/report";
import type { VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { CfContent, lookStyle } from "./CfCell";
import { fmt, numberAt, projectionFormat } from "./common";
import { CF_KINDS, type CfKind, type CellLook, cellLook, cfColumn, cfKey, columnFormatting, stats } from "./conditional";
import { additive, type Interact } from "./interact";

/** Expanded row paths per matrix visual (view state, not saved). `all` expands
 * everything; `none` collapses everything. */
interface Expansion {
  open: Record<string, Set<string>>;
  toggle(visual: string, path: string): void;
  setAll(visual: string, paths: string[]): void;
}

export const useExpansion = create<Expansion>()((set) => ({
  open: {},
  toggle: (visual, path) =>
    set((s) => {
      const next = new Set(s.open[visual] ?? []);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return { open: { ...s.open, [visual]: next } };
    }),
  setAll: (visual, paths) => set((s) => ({ open: { ...s.open, [visual]: new Set(paths) } })),
}));

interface Node {
  path: string;
  keys: (string | null)[];
  labels: string[];
  depth: number;
  /** Values by column member path ("" for the row total). */
  cells: Map<string, unknown[]>;
  children: Node[];
}

const pathOf = (keys: (string | null)[]) => JSON.stringify(keys);

interface Shaped {
  root: Node;
  columns: { path: string; labels: string[] }[];
  /** Paths of every row that has children (for Expand all). */
  parents: string[];
}

/** `extra` names more columns to read into each cell after the values (conditional formatting's fields). */
export function shapeMatrix(visual: Visual, model: Model, data: VisualData, blank: string, locale: string, extra: string[] = []): Shaped {
  const rows = visual.roles.rows ?? [];
  const cols = visual.roles.columns ?? [];
  const nValues = visual.roles.values?.length ?? 0;
  const rowFmt = rows.map((p) => projectionFormat(model, p));
  const colFmt = cols.map((p) => projectionFormat(model, p));
  const get = (name: string, r: number) => data.columns.get(name)?.get(r);
  const text = (v: unknown, f: (typeof rowFmt)[number]) => (v === null || v === undefined ? blank : fmt(v, f, locale));
  const keyAt = (name: string, r: number) => {
    const v = get(`key:${name}`, r);
    return v === null || v === undefined ? null : String(v);
  };

  // Rows of each level, in the order the core sent them (sorted within each level).
  const order: number[] = [];
  for (let r = 0; r < data.rows; r++) order.push(r);
  order.sort((a, b) => Number(get("level:rows", a)) - Number(get("level:rows", b)) || Number(get("level:columns", a)) - Number(get("level:columns", b)) || Number(get("order", a)) - Number(get("order", b)));

  const root: Node = { path: pathOf([]), keys: [], labels: [], depth: 0, cells: new Map(), children: [] };
  const nodes = new Map<string, Node>([[root.path, root]]);
  const columns: Shaped["columns"] = [];
  const seenCols = new Set<string>();
  for (const r of order) {
    const lr = Number(get("level:rows", r));
    const lc = Number(get("level:columns", r));
    const keys: (string | null)[] = [];
    const labels: string[] = [];
    for (let i = 0; i < lr; i++) {
      keys.push(keyAt(`rows:${i}`, r));
      labels.push(text(get(`rows:${i}`, r), rowFmt[i]!));
    }
    const path = pathOf(keys);
    let node = nodes.get(path);
    if (!node) {
      const parent = nodes.get(pathOf(keys.slice(0, -1)));
      if (!parent) continue;
      node = { path, keys, labels, depth: lr, cells: new Map(), children: [] };
      nodes.set(path, node);
      parent.children.push(node);
    }
    // Only leaf column members (and the row total) become columns.
    if (lc !== 0 && lc !== cols.length) continue;
    const ckeys: (string | null)[] = [];
    const clabels: string[] = [];
    for (let i = 0; i < lc; i++) {
      ckeys.push(keyAt(`columns:${i}`, r));
      clabels.push(text(get(`columns:${i}`, r), colFmt[i]!));
    }
    const cpath = lc === 0 ? "" : pathOf(ckeys);
    if (lc !== 0 && lr === 0 && !seenCols.has(cpath)) {
      seenCols.add(cpath);
      columns.push({ path: cpath, labels: clabels });
    }
    const values: unknown[] = [];
    for (let v = 0; v < nValues; v++) values.push(get(`values:${v}`, r));
    for (const name of extra) values.push(get(name, r));
    node.cells.set(cpath, values);
  }
  // Columns only seen below the grand total (no values there) still need a place.
  if (cols.length && !columns.length) {
    for (const r of order) {
      if (Number(get("level:columns", r)) !== cols.length) continue;
      const ckeys = cols.map((_, i) => keyAt(`columns:${i}`, r));
      const cpath = pathOf(ckeys);
      if (seenCols.has(cpath)) continue;
      seenCols.add(cpath);
      columns.push({ path: cpath, labels: cols.map((_, i) => text(get(`columns:${i}`, r), colFmt[i]!)) });
    }
  }
  const parents = [...nodes.values()].filter((n) => n.depth > 0 && n.children.length).map((n) => n.path);
  return { root, columns, parents };
}

export function Matrix({ visual, model, data, interact }: { visual: Visual; model: Model; data: VisualData; interact: Interact }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const def = visualDef("matrix");
  const open = useExpansion((s) => s.open[visual.id]);
  const toggle = useExpansion((s) => s.toggle);
  const rows = visual.roles.rows ?? [];
  const cols = visual.roles.columns ?? [];
  const values = visual.roles.values ?? [];
  const vfmt = values.map((p) => projectionFormat(model, p));
  // Conditional formatting's own fields come after the values in each cell.
  const cfs = useMemo(() => values.map((p) => columnFormatting(visual, p)), [values, visual]);
  const extra = useMemo(() => values.flatMap((p, v) => CF_KINDS.filter((k) => cfs[v]![k]?.field).map((k) => cfColumn(cfKey(p), k))), [values, cfs]);
  const shaped = useMemo(() => shapeMatrix(visual, model, data, t("(Blank)"), locale, extra), [visual, model, data, t, locale, extra]);
  const fontSize = prop<number>(visual.objects, def, "grid", "fontSize");
  const banded = prop<boolean>(visual.objects, def, "grid", "banded");
  const rowTotals = prop<boolean>(visual.objects, def, "totals", "rows");
  const colTotals = prop<boolean>(visual.objects, def, "totals", "columns") && cols.length > 0;
  const showSub = prop<boolean>(visual.objects, def, "totals", "subtotals");

  // Visible rows: depth-first through expanded nodes.
  const visible: Node[] = [];
  const walk = (n: Node) => {
    for (const c of n.children) {
      visible.push(c);
      if (open?.has(c.path)) walk(c);
    }
  };
  walk(shaped.root);
  const colPaths = [...shaped.columns.map((c) => c.path), ...(colTotals ? [""] : [])];
  const valueHeads = values.map((p) => projectionName(model, p, t));
  // The number a kind is based on in a cell, and the range over the cells shown (values
  // only, not totals, as Power BI applies it by default).
  const basedOn = (n: Node, cpath: string, v: number, k: CfKind): number | null => {
    const cells = n.cells.get(cpath);
    const cf = cfs[v]!;
    if (cf[k]?.field) {
      const name = cfColumn(cfKey(values[v]!), k);
      return numberAt({ get: (i: number) => cells?.[i] }, values.length + extra.indexOf(name));
    }
    return numberAt({ get: (i: number) => cells?.[i] }, v);
  };
  const shownRows = visible.filter((n) => !open?.has(n.path) || !n.children.length);
  const leafCols = shaped.columns.length ? shaped.columns.map((c) => c.path) : [""];
  const ranges = values.map((_, v) => new Map(CF_KINDS.filter((k) => cfs[v]![k]).map((k) => [k, stats(shownRows.flatMap((n) => leafCols.map((cp) => basedOn(n, cp, v, k))))])));
  const look = (n: Node, cpath: string, v: number): CellLook | null => {
    const cf = cfs[v]!;
    if (!CF_KINDS.some((k) => cf[k]) || n.depth === 0 || cpath === "" && cols.length > 0) return null;
    return cellLook(cf, (k) => basedOn(n, cpath, v, k), (k) => ranges[v]?.get(k) ?? null);
  };
  const cell = (n: Node, cpath: string, v: number) => {
    const raw = n.cells.get(cpath)?.[v];
    return raw === null || raw === undefined ? "" : fmt(raw, vfmt[v]!, locale);
  };
  const headerDepth = Math.max(1, cols.length) + (values.length > 1 || !cols.length ? 1 : 0);

  return (
    <div className={`table-visual matrix-visual ${banded ? "is-banded" : ""} has-lines`} style={{ fontSize }}>
      <table>
        <thead>
          {cols.length > 0 &&
            cols.map((_, level) => (
              <tr key={level}>
                {level === 0 && <th rowSpan={headerDepth} className="matrix-corner">{rows.map((p) => projectionName(model, p, t)).join(" / ")}</th>}
                {groupHeads(shaped.columns, level).map((g, i) => (
                  <th key={i} colSpan={g.span * Math.max(1, values.length)} className="is-num matrix-col-head">
                    {g.label}
                  </th>
                ))}
                {colTotals && level === 0 && (
                  <th rowSpan={cols.length} colSpan={Math.max(1, values.length)} className="is-num matrix-col-head">
                    {t("Total")}
                  </th>
                )}
              </tr>
            ))}
          {(values.length > 1 || !cols.length) && (
            <tr>
              {!cols.length && <th className="matrix-corner">{rows.map((p) => projectionName(model, p, t)).join(" / ")}</th>}
              {colPaths.flatMap((cp) =>
                valueHeads.map((h, v) => (
                  <th key={`${cp}${v}`} className="is-num">
                    {h}
                  </th>
                )),
              )}
            </tr>
          )}
        </thead>
        <tbody>
          {visible.map((n) => {
            const hasKids = n.children.length > 0 && n.depth < rows.length;
            const expanded = open?.has(n.path);
            const chosen = interact.chosen(n.keys);
            return (
              <tr key={n.path} className={`${expanded && showSub ? "is-subtotal" : ""} ${chosen === false ? "is-dim" : ""} ${chosen ? "is-chosen" : ""}`}>
                <th
                  scope="row"
                  className="matrix-row-head"
                  style={{ paddingInlineStart: 8 + (n.depth - 1) * 16 }}
                  onClick={(e) => interact.choose(rows.slice(0, n.depth), n.keys, additive(e))}
                  onContextMenu={(e) => {
                    if (!interact.menu) return;
                    e.preventDefault();
                    interact.menu(rows.slice(0, n.depth), n.keys, { x: e.clientX, y: e.clientY });
                  }}
                >
                  {hasKids ? (
                    <button
                      type="button"
                      className="matrix-toggle"
                      aria-label={expanded ? t("Collapse %1", n.labels.at(-1) ?? "") : t("Expand %1", n.labels.at(-1) ?? "")}
                      aria-expanded={!!expanded}
                      onClick={(e) => {
                        e.stopPropagation();
                        toggle(visual.id, n.path);
                      }}
                    >
                      {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                    </button>
                  ) : (
                    <span className="matrix-toggle-space" />
                  )}
                  {n.labels.at(-1)}
                </th>
                {colPaths.flatMap((cp) =>
                  values.map((_, v) => {
                    const lk = expanded ? null : look(n, cp, v);
                    return (
                      <td key={`${cp}${v}`} className={`is-num tabular ${lk?.bar ? "has-bar" : ""}`} style={lookStyle(lk)}>
                        <CfContent look={lk}>{expanded && !showSub ? "" : cell(n, cp, v)}</CfContent>
                      </td>
                    );
                  }),
                )}
              </tr>
            );
          })}
        </tbody>
        {rowTotals && values.length > 0 && (
          <tfoot>
            <tr>
              <th scope="row" className="matrix-row-head">
                {t("Total")}
              </th>
              {colPaths.flatMap((cp) =>
                values.map((_, v) => (
                  <td key={`${cp}${v}`} className="is-num tabular">
                    {cell(shaped.root, cp, v)}
                  </td>
                )),
              )}
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  );
}

/** Column header cells at one level of the column hierarchy: runs of members that
 * share their labels down to that level. */
function groupHeads(columns: { labels: string[] }[], level: number): { label: string; span: number }[] {
  const out: { label: string; span: number; prefix: string }[] = [];
  for (const c of columns) {
    const prefix = JSON.stringify(c.labels.slice(0, level + 1));
    const last = out.at(-1);
    if (last && last.prefix === prefix) last.span += 1;
    else out.push({ label: c.labels[level] ?? "", span: 1, prefix });
  }
  return out;
}
