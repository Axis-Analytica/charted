// A report theme's visual styles as format defaults. Power BI themes set them as
// `visualStyles[visualType | "*"]["*"][object][0][property]`, plus text classes
// (`textClasses.title.fontSize`…); charted reads the ones its own format objects have
// (mapped to its names) and inserts them between a visual's own formatting and the
// registry's defaults. Anything else in the theme is kept, and ignored.
import { fontStack } from "../design/fontNames";

/** Format defaults by charted visual type (or "*"), object and property. */
export type StyleDefaults = Record<string, Record<string, Record<string, unknown>>>;

/** charted visual types whose Power BI names differ. */
const POWER_BI_TYPE: Record<string, string> = {
  columnChart: "clusteredColumnChart",
  barChart: "clusteredBarChart",
  stackedColumnChart: "columnChart",
  stackedBarChart: "barChart",
  table: "tableEx",
  matrix: "pivotTable",
};
const CHARTED_TYPE: Record<string, string> = Object.fromEntries(Object.entries(POWER_BI_TYPE).map(([c, p]) => [p, c]));

/** A visual type's Power BI name. */
export const powerBiType = (type: string) => POWER_BI_TYPE[type] ?? type;

type Value = unknown;
type Mapping = [object: string, prop: string, convert?: (v: Value) => Value];

const bool = (v: Value) => (typeof v === "boolean" ? v : undefined);
const num = (v: Value) => (typeof v === "number" && Number.isFinite(v) ? v : undefined);
const lower = (v: Value) => (typeof v === "string" ? v.toLowerCase() : undefined);
/** A colour: `"#rrggbb"` or Power BI's `{ solid: { color } }`. */
export const color = (v: Value): string | undefined => {
  const c = typeof v === "object" && v !== null ? (v as { solid?: { color?: unknown } }).solid?.color : v;
  return typeof c === "string" && /^#[0-9a-f]{3,8}$/i.test(c) ? c : undefined;
};
/** Power BI's legend positions (TopCenter, RightCenter…) as charted's. */
const position = (v: Value) => {
  const s = typeof v === "string" ? v.toLowerCase() : "";
  return s.startsWith("top") ? "top" : s.startsWith("bottom") ? "bottom" : s.startsWith("right") ? "right" : undefined;
};

/** Power BI object → property → where it goes in charted. */
const COMMON: Record<string, Record<string, Mapping>> = {
  title: { show: ["title", "show", bool], fontSize: ["title", "fontSize", num], alignment: ["title", "align", lower] },
  legend: { show: ["legend", "show", bool], position: ["legend", "position", position] },
  categoryAxis: { show: ["categoryAxis", "show", bool], showAxisTitle: ["categoryAxis", "title", bool], gridlineShow: ["categoryAxis", "gridlines", bool] },
  valueAxis: { show: ["valueAxis", "show", bool], showAxisTitle: ["valueAxis", "title", bool], gridlineShow: ["valueAxis", "gridlines", bool] },
  labels: { show: ["dataLabels", "show", bool], fontSize: ["dataLabels", "fontSize", num] },
  background: { show: ["background", "show", bool] },
  border: { show: ["background", "border", bool] },
  dataPoint: { defaultColor: ["colors", "fill", color] },
  lineStyles: { strokeWidth: ["lines", "width", num], showMarker: ["lines", "markers", bool] },
  total: { totals: ["totals", "show", bool] },
};
/** Where a visual type's objects mean something else. */
const BY_TYPE: Record<string, Record<string, Record<string, Mapping>>> = {
  card: { labels: { fontSize: ["callout", "fontSize", num] }, categoryLabels: { show: ["label", "show", bool] } },
  multiRowCard: { dataLabels: { fontSize: ["callout", "fontSize", num] }, categoryLabels: { show: ["label", "show", bool] } },
  tableEx: { values: { fontSize: ["grid", "fontSize", num] }, total: { totals: ["totals", "show", bool] } },
  pivotTable: { values: { fontSize: ["grid", "fontSize", num] } },
};

function put(out: StyleDefaults, type: string, object: string, prop: string, value: Value) {
  if (value === undefined) return;
  ((out[type] ??= {})[object] ??= {})[prop] = value;
}

/** One `visualStyles[type]["*"]` entry into charted's defaults for `chartedType`. */
function readObjects(out: StyleDefaults, chartedType: string, powerBi: string, objects: unknown) {
  if (typeof objects !== "object" || objects === null) return;
  for (const [object, list] of Object.entries(objects as Record<string, unknown>)) {
    const props = Array.isArray(list) ? list[0] : list;
    if (typeof props !== "object" || props === null) continue;
    for (const [prop, value] of Object.entries(props as Record<string, unknown>)) {
      const m = BY_TYPE[powerBi]?.[object]?.[prop] ?? COMMON[object]?.[prop];
      if (!m) continue;
      const [o, p, convert] = m;
      put(out, chartedType, o, p, convert ? convert(value) : value);
    }
  }
}

/** The format defaults a theme's text classes and visual styles give. */
export function styleDefaults(json: unknown): StyleDefaults {
  const out: StyleDefaults = {};
  if (typeof json !== "object" || json === null) return out;
  const j = json as { textClasses?: Record<string, { fontSize?: unknown }>; visualStyles?: Record<string, Record<string, unknown>> };
  // Text classes first: visual styles override them.
  const tc = j.textClasses ?? {};
  put(out, "*", "title", "fontSize", num(tc.title?.fontSize));
  put(out, "*", "callout", "fontSize", num(tc.callout?.fontSize));
  put(out, "*", "dataLabels", "fontSize", num(tc.label?.fontSize));
  put(out, "*", "grid", "fontSize", num(tc.label?.fontSize));
  const vs = j.visualStyles ?? {};
  readObjects(out, "*", "*", vs["*"]?.["*"]);
  for (const [type, styles] of Object.entries(vs)) {
    if (type === "*") continue;
    readObjects(out, CHARTED_TYPE[type] ?? type, type, styles?.["*"]);
  }
  return out;
}

let active: StyleDefaults = {};

/** Makes `defaults` the ones `themeDefault` reads (the report's current theme). */
export function setStyleDefaults(defaults: StyleDefaults) {
  active = defaults;
}

/** The theme's default for a visual type's format property, if it sets one. */
export function themeDefault(visualType: string, object: string, prop: string, from: StyleDefaults = active): unknown {
  return from[visualType]?.[object]?.[prop] ?? from["*"]?.[object]?.[prop];
}

/** CSS custom properties for the theme's text classes that charted styles directly:
 * visual titles' colour and font, and the page's font. */
export function textTokens(json: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  if (typeof json !== "object" || json === null) return out;
  const tc = ((json as { textClasses?: Record<string, { color?: unknown; fontFace?: unknown }> }).textClasses ?? {}) as Record<string, { color?: unknown; fontFace?: unknown }>;
  const font = fontStack;
  const set = (k: string, v: string | undefined) => v && (out[k] = v);
  set("--report-title-fg", color(tc.title?.color));
  set("--report-title-font", font(tc.title?.fontFace));
  set("--report-font", font(tc.label?.fontFace));
  set("--report-callout-font", font((tc as Record<string, { fontFace?: unknown }>).callout?.fontFace));
  return out;
}
