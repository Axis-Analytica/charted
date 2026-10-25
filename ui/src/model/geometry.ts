// Card sizes and relationship-line geometry for the model view, kept free of React so
// the layout worker and tests can use it.
import type { Relationship } from "../bridge/generated/Relationship";
import type { Table } from "../bridge/generated/Table";

export const CARD_W = 232;
export const HEAD = 36;
export const ROW = 24;
export const PAD = 6;

export interface Point {
  x: number;
  y: number;
}

export function cardHeight(t: Table): number {
  return HEAD + PAD * 2 + t.columns.length * ROW + (t.measures.length ? ROW : 0);
}

/** The vertical middle of a column's row, relative to the card. */
export function rowY(t: Table, columnId: string): number {
  const i = Math.max(0, t.columns.findIndex((c) => c.id === columnId));
  return HEAD + PAD + i * ROW + ROW / 2;
}

export interface Edge {
  path: string;
  from: Point & { dir: number };
  to: Point & { dir: number };
  /** Middle of the curve and the unit direction there (from → to). */
  mid: Point;
  tangent: Point;
}

function bezier(p0: Point, p1: Point, p2: Point, p3: Point, t: number): Point {
  const u = 1 - t;
  return {
    x: u * u * u * p0.x + 3 * u * u * t * p1.x + 3 * u * t * t * p2.x + t * t * t * p3.x,
    y: u * u * u * p0.y + 3 * u * u * t * p1.y + 3 * u * t * t * p2.y + t * t * t * p3.y,
  };
}

/** A curve from the from-column's row to the to-column's row, leaving each card on the
 * side facing the other. */
export function edge(r: Relationship, tables: Map<string, Table>, pos: Map<string, Point>): Edge | null {
  const ft = tables.get(r.fromTable);
  const tt = tables.get(r.toTable);
  const fp = pos.get(r.fromTable);
  const tp = pos.get(r.toTable);
  if (!ft || !tt || !fp || !tp) return null;
  const fc = fp.x + CARD_W / 2;
  const tc = tp.x + CARD_W / 2;
  let fdir: number;
  let tdir: number;
  if (Math.abs(fc - tc) < CARD_W * 0.6) {
    // Stacked cards: loop out of the right sides.
    fdir = 1;
    tdir = 1;
  } else {
    fdir = fc < tc ? 1 : -1;
    tdir = -fdir;
  }
  const from = { x: fdir > 0 ? fp.x + CARD_W : fp.x, y: fp.y + rowY(ft, r.fromColumn), dir: fdir };
  const to = { x: tdir > 0 ? tp.x + CARD_W : tp.x, y: tp.y + rowY(tt, r.toColumn), dir: tdir };
  const reach = Math.max(48, Math.min(160, Math.abs(to.x - from.x) / 2));
  const c1 = { x: from.x + fdir * reach, y: from.y };
  const c2 = { x: to.x + tdir * reach, y: to.y };
  const mid = bezier(from, c1, c2, to, 0.5);
  const a = bezier(from, c1, c2, to, 0.48);
  const b = bezier(from, c1, c2, to, 0.52);
  const len = Math.hypot(b.x - a.x, b.y - a.y) || 1;
  return {
    path: `M ${from.x} ${from.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${to.x} ${to.y}`,
    from,
    to,
    mid,
    tangent: { x: (b.x - a.x) / len, y: (b.y - a.y) / len },
  };
}

/** A filled triangle at `p` pointing along `d`. */
export function arrow(p: Point, d: Point, size = 6): string {
  const n = { x: -d.y, y: d.x };
  const tip = { x: p.x + d.x * size, y: p.y + d.y * size };
  const l = { x: p.x - d.x * size + n.x * size * 0.8, y: p.y - d.y * size + n.y * size * 0.8 };
  const rr = { x: p.x - d.x * size - n.x * size * 0.8, y: p.y - d.y * size - n.y * size * 0.8 };
  return `M ${tip.x} ${tip.y} L ${l.x} ${l.y} L ${rr.x} ${rr.y} Z`;
}
