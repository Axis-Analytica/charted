// Shared helpers for visual renderers: how each projection's values are formatted.
import type { DataType } from "../../bridge/generated/DataType";
import type { Model } from "../../bridge/generated/Model";
import type { Projection } from "../../bridge/generated/Projection";
import { formatValue } from "../../format";
import { columnOf } from "../../state/report";

export interface FieldFormat {
  type: DataType;
  format: string | null;
}

/** The type and format string a projection's values are shown with. */
export function projectionFormat(model: Model, p: Projection): FieldFormat {
  if (p.field.kind === "measure") {
    const f = p.field;
    const m = model.tables.find((t) => t.id === f.table)?.measures.find((x) => x.id === f.measure);
    return { type: "Double", format: m?.formatString ?? null };
  }
  const c = columnOf(model, p.field);
  if (!c) return { type: "String", format: null };
  switch (p.aggregation) {
    case "count":
    case "distinctCount":
      return { type: "Int64", format: "#,0" };
    case "average":
      return { type: "Double", format: c.formatString ?? "#,0.00" };
    case "sum":
      return { type: c.dataType === "Int64" ? "Int64" : "Double", format: c.formatString ?? (c.dataType === "Int64" ? "#,0" : null) };
    default:
      return { type: c.dataType, format: c.formatString ?? null };
  }
}

export function fmt(v: unknown, f: FieldFormat, locale: string): string {
  return formatValue(typeof v === "bigint" ? Number(v) : v, f.type, f.format, locale);
}

/** Display units for a big number: "auto" picks K, M or B by size, like Power BI. */
export function withUnits(v: number, units: string, f: FieldFormat, locale: string): string {
  const abs = Math.abs(v);
  const unit = units === "auto" ? (abs >= 1e9 ? "B" : abs >= 1e6 ? "M" : abs >= 1e4 ? "K" : "none") : units;
  const div = unit === "K" ? 1e3 : unit === "M" ? 1e6 : unit === "B" ? 1e9 : 1;
  if (div === 1) return fmt(v, f, locale);
  // Keep the format's currency prefix and percent sign, but show one or two decimals.
  const scaled = v / div;
  const body = new Intl.NumberFormat(locale, { maximumFractionDigits: Math.abs(scaled) < 10 ? 2 : 1 }).format(scaled);
  const prefix = /^\\?\$|^"\$"/.test(f.format ?? "") ? "$" : "";
  return `${scaled < 0 && prefix ? "-" : ""}${prefix}${prefix ? body.replace("-", "") : body}${unit}`;
}

/** Compact axis labels (1.2K, 3.4M), keeping a currency sign. */
export function axisLabel(v: number, f: FieldFormat, locale: string): string {
  if (f.format && /%/.test(f.format.replace(/\\%|"[^"]*"/g, ""))) return fmt(v, f, locale);
  const prefix = /^\\?\$|^"\$"/.test(f.format ?? "") ? "$" : "";
  const body = new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 }).format(Math.abs(v));
  return `${v < 0 ? "-" : ""}${prefix}${body}`;
}

/** The live chart palette from the theme tokens (the report theme's when `el` is on a
 * themed page). */
export function palette(el?: Element | null): string[] {
  const s = getComputedStyle(el ?? document.documentElement);
  const out: string[] = [];
  // A report theme with fewer colours marks where its list ends.
  const n = Number(s.getPropertyValue("--chart-count").trim()) || 10;
  for (let i = 1; i <= n; i++) {
    const c = s.getPropertyValue(`--chart-${i}`).trim();
    if (c) out.push(c);
  }
  return out.length ? out : ["#4e79a7", "#f28e2b", "#59a14f", "#e15759"];
}

export function token(name: string, el?: Element | null): string {
  return getComputedStyle(el ?? document.documentElement).getPropertyValue(name).trim();
}

const rgbCache = new Map<string, [number, number, number]>();

/** `color` (any CSS colour) at `alpha` (0–1), as `rgba()` for the chart canvas. */
export function fade(color: string, alpha: number): string {
  let rgb = rgbCache.get(color);
  if (!rgb) {
    const c = document.createElement("canvas");
    c.width = c.height = 1;
    const ctx = c.getContext("2d", { willReadFrequently: true });
    rgb = [136, 136, 136];
    if (ctx) {
      ctx.fillStyle = color;
      ctx.fillRect(0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      rgb = [d[0] ?? 0, d[1] ?? 0, d[2] ?? 0];
    }
    rgbCache.set(color, rgb);
  }
  return `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, ${alpha})`;
}

/** A cell of an Arrow column as a number, null for blank. */
export function numberAt(v: { get(i: number): unknown } | undefined, r: number): number | null {
  const x = v?.get(r);
  return x === null || x === undefined ? null : Number(x);
}
