// Conditional formatting for tables and matrices: a cell's background or font colour, a
// data bar, or an icon, from rules (value ranges) or a gradient over the column's values,
// based on the cell's own value or on another field's (a hidden value of the query,
// `cf:{column}:{kind}`). Stored in `objects.conditionalFormatting[column][kind]`, where
// the column is a field key (so it follows the field when columns move).
import type { FieldRef } from "../../bridge/generated/FieldRef";
import type { Projection } from "../../bridge/generated/Projection";
import type { Visual } from "../../bridge/generated/Visual";

// As state/report's `fieldKey` (kept free of the app's state, for tests).
const fieldKey = (f: FieldRef) => (f.kind === "column" ? `c:${f.table}:${f.column}` : `m:${f.table}:${f.measure}`);

export type CfKind = "background" | "fontColor" | "dataBars" | "icons";
export const CF_KINDS: CfKind[] = ["background", "fontColor", "dataBars", "icons"];

export interface CfBound {
  kind: "lowest" | "highest" | "number";
  value: number;
}

export interface CfRuleLine {
  /** Inclusive lower and exclusive upper bound; null for open. */
  from: number | null;
  to: number | null;
  /** Bounds as numbers, or as percents of the range (0–100). */
  unit: "number" | "percent";
  color: string;
  icon?: CfIcon;
}

export type CfIcon = "up" | "right" | "down" | "good" | "warn" | "bad" | "flag" | "star";

export interface CfRule {
  mode: "gradient" | "rules";
  /** The field the formatting is based on; null for the cell's own value. */
  field: Projection | null;
  min: CfBound;
  max: CfBound;
  /** A diverging gradient's middle (null for two colours). */
  mid: CfBound | null;
  minColor: string;
  midColor: string;
  maxColor: string;
  rules: CfRuleLine[];
  /** Data bars: colours for positive and negative values, and whether to hide the value. */
  positive?: string;
  negative?: string;
  barOnly?: boolean;
}

export type CfColumn = Partial<Record<CfKind, CfRule>>;

/** The key a column's formatting is stored under. */
export const cfKey = (p: Projection) => `${fieldKey(p.field)}|${p.aggregation ?? ""}`;

/** The hidden query column holding a rule's field values (as visualq names it). */
export const cfColumn = (key: string, kind: CfKind) => `cf:${key}:${kind}`;

export function columnFormatting(visual: Visual, p: Projection): CfColumn {
  const all = visual.objects?.conditionalFormatting as Record<string, CfColumn> | undefined;
  return all?.[cfKey(p)] ?? {};
}

export function defaultRule(kind: CfKind, accent: string, good: string, bad: string, warn: string): CfRule {
  const base: CfRule = {
    mode: kind === "icons" ? "rules" : "gradient",
    field: null,
    min: { kind: "lowest", value: 0 },
    max: { kind: "highest", value: 0 },
    mid: null,
    minColor: kind === "fontColor" ? bad : "#f4f6fa",
    midColor: warn,
    maxColor: kind === "fontColor" ? good : accent,
    rules: [],
  };
  if (kind === "icons")
    base.rules = [
      { from: 67, to: null, unit: "percent", color: good, icon: "up" },
      { from: 33, to: 67, unit: "percent", color: warn, icon: "right" },
      { from: null, to: 33, unit: "percent", color: bad, icon: "down" },
    ];
  if (kind === "dataBars") Object.assign(base, { positive: accent, negative: bad, barOnly: false });
  return base;
}

/** The smallest and largest of the values a rule is based on. */
export interface Stats {
  min: number;
  max: number;
}

export function stats(values: Iterable<number | null>): Stats | null {
  let min = Infinity;
  let max = -Infinity;
  for (const v of values) {
    if (v === null || !Number.isFinite(v)) continue;
    min = Math.min(min, v);
    max = Math.max(max, v);
  }
  return min === Infinity ? null : { min, max };
}

const bound = (b: CfBound, s: Stats) => (b.kind === "lowest" ? s.min : b.kind === "highest" ? s.max : b.value);

function rgb(c: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(c.trim());
  if (!m) return null;
  const h = m[1]!.length === 3 ? [...m[1]!].map((x) => x + x).join("") : m[1]!;
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

/** `a` to `b` at `t` (0–1), for hex colours. */
export function mix(a: string, b: string, t: number): string {
  const x = rgb(a);
  const y = rgb(b);
  if (!x || !y) return t < 0.5 ? a : b;
  const c = x.map((v, i) => Math.round(v + (y[i]! - v) * Math.min(1, Math.max(0, t))));
  return `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
}

/** The colour a gradient gives `v`. */
export function gradientColor(r: CfRule, v: number, s: Stats): string {
  const lo = bound(r.min, s);
  const hi = bound(r.max, s);
  if (r.mid) {
    const m = bound(r.mid, s);
    if (v <= m) return mix(r.minColor, r.midColor, m === lo ? 1 : (v - lo) / (m - lo));
    return mix(r.midColor, r.maxColor, hi === m ? 1 : (v - m) / (hi - m));
  }
  return mix(r.minColor, r.maxColor, hi === lo ? 1 : (v - lo) / (hi - lo));
}

/** The first rule line `v` falls in. */
export function ruleFor(r: CfRule, v: number, s: Stats | null): CfRuleLine | null {
  for (const line of r.rules) {
    const scale = (x: number | null) => (x === null ? null : line.unit === "percent" ? (s ? s.min + ((s.max - s.min) * x) / 100 : x) : x);
    const from = scale(line.from);
    const to = scale(line.to);
    if ((from === null || v >= from) && (to === null || v < to || (line.unit === "percent" && line.to === 100 && v <= to))) return line;
  }
  return null;
}

export interface CellLook {
  background?: string;
  color?: string;
  bar?: { from: number; to: number; color: string };
  icon?: { name: CfIcon; color: string };
  hideValue?: boolean;
}

/** How a cell looks under a column's formatting: `value(kind)` is the number each rule
 * is based on, `stat(kind)` the range of those numbers over the visual. */
export function cellLook(cf: CfColumn, value: (k: CfKind) => number | null, stat: (k: CfKind) => Stats | null): CellLook {
  const out: CellLook = {};
  for (const k of CF_KINDS) {
    const r = cf[k];
    if (!r) continue;
    const v = value(k);
    if (v === null || !Number.isFinite(v)) continue;
    const s = stat(k);
    if (k === "dataBars") {
      if (!s) continue;
      const lo = Math.min(0, bound(r.min, s));
      const hi = Math.max(0, bound(r.max, s));
      const span = hi - lo || 1;
      const zero = ((0 - lo) / span) * 100;
      const at = ((Math.min(hi, Math.max(lo, v)) - lo) / span) * 100;
      out.bar = { from: Math.min(zero, at), to: Math.max(zero, at), color: v < 0 ? (r.negative ?? "#d0453a") : (r.positive ?? "#4e79a7") };
      out.hideValue = !!r.barOnly;
      continue;
    }
    let color: string | undefined;
    let icon: CfIcon | undefined;
    if (r.mode === "gradient" && k !== "icons") {
      if (s) color = gradientColor(r, v, s);
    } else {
      const line = ruleFor(r, v, s);
      color = line?.color;
      icon = line?.icon;
    }
    if (k === "background" && color) out.background = color;
    if (k === "fontColor" && color) out.color = color;
    if (k === "icons" && icon) out.icon = { name: icon, color: color ?? "currentColor" };
  }
  return out;
}

/** Readable text on a coloured background. */
export function contrastText(bg: string): string | undefined {
  const m = /rgb\((\d+), (\d+), (\d+)\)/.exec(bg);
  const c = m ? [Number(m[1]), Number(m[2]), Number(m[3])] : rgb(bg);
  if (!c) return undefined;
  const l = (0.299 * c[0]! + 0.587 * c[1]! + 0.114 * c[2]!) / 255;
  return l > 0.6 ? "#1b1d21" : "#ffffff";
}
