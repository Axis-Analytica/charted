// Clustered, stacked and 100% stacked column and bar charts, line charts, and line and
// column combo charts (ECharts, canvas). Categories come from the category role; series
// from the legend role (one value pivoted by it) or from each value field; a combo
// chart's line values are lines over the columns, on a secondary axis. A small
// multiples field splits the chart into a grid of small charts sharing their axes.
// Clicking a bar or a point on a line chooses it (Ctrl adds to the choice); a highlight
// from elsewhere draws each bar's highlighted part solid and the rest faded.
import { BarChart, LineChart } from "echarts/charts";
import { GridComponent, LegendComponent, MarkLineComponent, TitleComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo, useRef } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Projection } from "../../bridge/generated/Projection";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { hierarchy } from "../../state/drill";
import { projectionName } from "../../state/report";
import { keysOf, type VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { CanvasScale, ThemeKey } from "../scale";
import { axisLabel, type FieldFormat, fade, fmt, numberAt, palette, projectionFormat, token } from "./common";
import { trend } from "./calc";
import { additive, type Interact } from "./interact";

echarts.use([BarChart, LineChart, GridComponent, TooltipComponent, LegendComponent, MarkLineComponent, TitleComponent, CanvasRenderer]);

/** Series beyond this many (by legend value) are left out, as Power BI's top 60. */
const MAX_SERIES = 60;
/** Small multiples beyond this many are left out. */
const MAX_PANELS = 36;

interface SeriesMeta {
  name: string;
  /** The legend value's key, when series come from the legend role. */
  key: string | null;
  format: FieldFormat;
}

interface Series {
  values: (number | null)[];
  /** Highlighted parts, when a selection elsewhere highlights this chart. */
  hl: (number | null)[] | null;
}

/** One small multiple (or the whole chart): its series' values by category. */
interface Panel {
  name: string;
  key: string | null;
  series: Series[];
  /** A combo chart's line values. */
  lines: Series[];
}

/** The chart's variant, from its type. */
export function variant(kind: string) {
  const combo = kind === "lineClusteredColumnComboChart" || kind === "lineStackedColumnComboChart";
  const percent = kind === "hundredPercentStackedColumnChart" || kind === "hundredPercentStackedBarChart";
  return {
    combo,
    percent,
    stacked: percent || kind === "stackedColumnChart" || kind === "stackedBarChart" || kind === "lineStackedColumnComboChart",
    horizontal: kind === "barChart" || kind === "stackedBarChart" || kind === "hundredPercentStackedBarChart",
    isLine: kind === "lineChart",
  };
}

/** A "nice" axis bound at or beyond `v` (1, 2, 2.5 or 5 times a power of ten). */
function nice(v: number): number {
  if (v === 0) return 0;
  const sign = Math.sign(v);
  const a = Math.abs(v);
  const p = 10 ** Math.floor(Math.log10(a));
  const m = [1, 2, 2.5, 5, 10].find((x) => x * p >= a) ?? 10;
  return sign * m * p;
}


export function Cartesian({ visual, model, data, interact }: { visual: Visual; model: Model; data: VisualData; interact: Interact }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const themeKey = useContext(ThemeKey);
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const scale = useContext(CanvasScale);
  const def = visualDef(visual.visualType);
  const kind = visual.visualType;
  // The levels of the axis hierarchy shown (one, or several when expanded).
  const levels = useMemo(() => hierarchy(visual)?.shown ?? [], [visual]);
  const catFields = useMemo(() => levels.map((i) => visual.roles.category![i]!), [levels, visual.roles.category]);
  const cat = catFields[0];
  const ser = visual.roles.series?.[0];
  const mult = visual.roles.smallMultiples?.[0];
  const vals = useMemo(() => visual.roles.values ?? [], [visual.roles.values]);
  const lineVals = useMemo(() => visual.roles.lineValues ?? [], [visual.roles.lineValues]);
  const { combo, percent, stacked, horizontal, isLine } = variant(kind);

  const shaped = useMemo(() => {
    const catVecs = levels.map((i) => data.columns.get(`category:${i}`));
    const serVec = data.columns.get("series:0");
    const multVec = data.columns.get("smallMultiples:0");
    const catKeyCols = levels.map((i) => keysOf(data, `category:${i}`));
    const serKeys = ser ? keysOf(data, "series:0") : [];
    const multKeys = mult ? keysOf(data, "smallMultiples:0") : [];
    const catFmts = catFields.map((p) => projectionFormat(model, p));
    const serFmt = ser ? projectionFormat(model, ser) : null;
    const multFmt = mult ? projectionFormat(model, mult) : null;
    const valFmts = vals.map((p) => projectionFormat(model, p));
    const label = (v: unknown, f: FieldFormat | null) => (v === null || v === undefined ? t("(Blank)") : f ? fmt(v, f, locale) : String(v));
    const catLabel = (r: number) => catVecs.map((v, j) => label(v?.get(r), catFmts[j] ?? null)).join(" ");
    const catKey = (r: number) => catKeyCols.map((k) => k[r] ?? null);
    const categories: string[] = [];
    // Each category's values at the levels shown (what a selection filters on).
    const keys: (string | null)[][] = [];
    const catIndex = new Map<string, number>();
    const meta: SeriesMeta[] = [];
    const lineMeta: SeriesMeta[] = lineVals.map((p) => ({ name: projectionName(model, p, t), key: null, format: projectionFormat(model, p) }));
    const pivot = !!(ser && serVec && vals.length);
    if (!pivot) vals.forEach((p, i) => meta.push({ name: projectionName(model, p, t), key: null, format: valFmts[i]! }));
    const seriesIndex = new Map<string, number>();
    const panels = new Map<string, Panel>();
    const empty = (): Series => ({ values: [], hl: data.highlighted ? [] : null });
    for (let r = 0; r < data.rows; r++) {
      const ck = catKey(r);
      const cid = JSON.stringify(ck);
      if (!catIndex.has(cid)) {
        catIndex.set(cid, categories.length);
        categories.push(cat ? catLabel(r) : "");
        keys.push(ck);
      }
      const ci = catIndex.get(cid)!;
      const pk = mult ? (multKeys[r] ?? null) : null;
      let panel = panels.get(String(pk));
      if (!panel) {
        if (panels.size >= MAX_PANELS) continue;
        panel = { name: mult ? label(multVec?.get(r), multFmt) : "", key: pk, series: [], lines: lineMeta.map(empty) };
        panels.set(String(pk), panel);
      }
      const put = (s: Series, vec: string) => {
        s.values[ci] = numberAt(data.columns.get(vec), r);
        if (s.hl) s.hl[ci] = numberAt(data.columns.get(`hl:${vec}`), r);
      };
      if (pivot) {
        // Pivot the first value by the legend field.
        const sk = serKeys[r] ?? null;
        let si = seriesIndex.get(String(sk));
        if (si === undefined) {
          if (meta.length >= MAX_SERIES) continue;
          si = meta.length;
          seriesIndex.set(String(sk), si);
          meta.push({ name: label(serVec!.get(r), serFmt), key: sk, format: valFmts[0]! });
        }
        put((panel.series[si] ??= empty()), "values:0");
      } else {
        vals.forEach((_, i) => put((panel!.series[i] ??= empty()), `values:${i}`));
      }
      lineVals.forEach((_, j) => put(panel!.lines[j]!, `lineValues:${j}`));
    }
    const list = [...panels.values()];
    if (mult) list.sort((a, b) => (a.key === null ? 1 : b.key === null ? -1 : a.name.localeCompare(b.name, locale, { numeric: true })));
    for (const p of list) {
      for (let si = 0; si < meta.length; si++) p.series[si] ??= empty();
      for (const s of [...p.series, ...p.lines])
        for (let i = 0; i < categories.length; i++) {
          s.values[i] ??= null;
          if (s.hl) s.hl[i] ??= null;
        }
    }
    if (!list.length) list.push({ name: "", key: null, series: meta.map(empty), lines: lineMeta.map(empty) });
    return { categories, keys, meta, lineMeta, panels: list };
  }, [data, cat, catFields, levels, ser, mult, vals, lineVals, model, locale, t]);

  // The fields and values that identify a point.
  const point = (category: number, series: number | null, panel = 0): { fields: Projection[]; tuple: (string | null)[] } | null => {
    const fields: Projection[] = [...catFields];
    const tuple: (string | null)[] = catFields.map((_, j) => shaped.keys[category]?.[j] ?? null);
    const s = series === null ? null : shaped.meta[series];
    if (ser && s) {
      fields.push(ser);
      tuple.push(s.key);
    }
    if (mult) {
      fields.push(mult);
      tuple.push(shaped.panels[panel]?.key ?? null);
    }
    return fields.length ? { fields, tuple } : null;
  };
  const pageTipsOn = !!interact.hover;
  const pointRef = useRef(point);
  pointRef.current = point;
  const interactRef = useRef(interact);
  interactRef.current = interact;
  const keysRef = useRef(shaped.keys);
  keysRef.current = cat ? shaped.keys : [];
  const panelsRef = useRef(shaped.panels.length);
  panelsRef.current = shaped.panels.length;

  useEffect(() => {
    if (!el.current) return;
    // Under a scaled canvas, draw at enough pixels for the zoom.
    const c = echarts.init(el.current, undefined, { renderer: "canvas", devicePixelRatio: (window.devicePixelRatio || 1) * Math.max(1, scale) });
    chart.current = c;
    // Tests find points through the chart (`convertToPixel`).
    (el.current as unknown as { chart: echarts.ECharts }).chart = c;
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(el.current);
    let onItem = false;
    // Series ids are `kind:series:panel` (kind v, h or r for bars, l for lines).
    const idOf = (seriesIndex: number) => {
      const id = (c.getOption() as { series?: { id?: string }[] }).series?.[seriesIndex]?.id ?? "";
      const [k, s, p] = id.split(":");
      return { line: k === "l", series: k === "l" ? null : Number(s ?? 0), panel: Number(p ?? 0) };
    };
    // The grid (small multiple) under a pixel, and the category there.
    const at = (x: number, y: number): { panel: number; category: number } | null => {
      for (let g = 0; g < panelsRef.current; g++) {
        if (!c.containPixel({ gridIndex: g }, [x, y])) continue;
        const opt = c.getOption() as { yAxis?: { type?: string }[] };
        const horiz = opt.yAxis?.[0]?.type === "category";
        const v = c.convertFromPixel({ gridIndex: g }, [x, y]) as number[] | undefined;
        const i = Math.round(v?.[horiz ? 1 : 0] ?? -1);
        return i >= 0 && i < keysRef.current.length + (keysRef.current.length ? 0 : 1) ? { panel: g, category: i } : null;
      }
      return null;
    };
    // Drill mode: a click on a category drills into it.
    const drillAt = (category: number) => {
      const keys = keysRef.current[category];
      if (!interactRef.current.drill || !keys?.length) return false;
      interactRef.current.drill(keys);
      return true;
    };
    c.on("click", (p) => {
      const params = p as unknown as { componentType?: string; dataIndex: number; seriesIndex: number; event?: { event?: MouseEvent } };
      if (params.componentType !== "series") return;
      onItem = true;
      // Highlighted bars are drawn as pairs of series; both stand for the same one.
      const { series, panel } = idOf(params.seriesIndex);
      if (drillAt(params.dataIndex)) return;
      const pt = pointRef.current(params.dataIndex, series, panel);
      if (pt) interactRef.current.choose(pt.fields, pt.tuple, additive(params.event?.event));
    });
    c.on("contextmenu", (p) => {
      const params = p as unknown as { componentType?: string; dataIndex: number; seriesIndex: number; event?: { event?: MouseEvent } };
      if (params.componentType !== "series") return;
      onItem = true;
      const ev = params.event?.event;
      const { series, panel } = idOf(params.seriesIndex);
      const pt = pointRef.current(params.dataIndex, series, panel);
      if (!pt || !ev || !interactRef.current.menu) return;
      ev.preventDefault();
      interactRef.current.menu(pt.fields, pt.tuple, { x: ev.clientX, y: ev.clientY });
    });
    const isLines = () => (c.getOption() as { series?: { type?: string }[] }).series?.[0]?.type === "line";
    c.getZr().on("contextmenu", (e) => {
      if (onItem) {
        onItem = false;
        return;
      }
      // Lines: the category under the pointer.
      const ev = e.event as unknown as MouseEvent;
      const hit = isLines() ? at(e.offsetX, e.offsetY) : null;
      if (!hit || !interactRef.current.menu) return;
      const pt = pointRef.current(hit.category, null, hit.panel);
      if (!pt) return;
      ev.preventDefault();
      interactRef.current.menu(pt.fields, pt.tuple, { x: ev.clientX, y: ev.clientY });
    });
    c.getZr().on("click", (e) => {
      if (onItem) {
        onItem = false;
        return;
      }
      // Lines: the category under the pointer. Elsewhere: empty space clears.
      if (isLines()) {
        const hit = at(e.offsetX, e.offsetY);
        if (hit) {
          if (drillAt(hit.category)) return;
          const pt = pointRef.current(hit.category, null, hit.panel);
          if (pt) interactRef.current.choose(pt.fields, pt.tuple, additive(e.event as unknown as MouseEvent));
          return;
        }
      }
      interactRef.current.clear();
    });
    // A report page as the tooltip: the category (and bar) under the pointer.
    let hovered: number | null = null;
    c.on("mouseover", (p) => {
      const params = p as unknown as { componentType?: string; seriesIndex: number };
      if (params.componentType === "series") hovered = idOf(params.seriesIndex).series;
    });
    c.on("mouseout", () => {
      hovered = null;
    });
    c.getZr().on("mousemove", (e) => {
      const hover = interactRef.current.hover;
      if (!hover) return;
      const ev = e.event as unknown as MouseEvent;
      const hit = at(e.offsetX, e.offsetY);
      const pt = hit ? pointRef.current(hit.category, hovered, hit.panel) : null;
      hover(pt?.fields ?? null, pt?.tuple ?? [], { x: ev.clientX, y: ev.clientY });
    });
    c.getZr().on("globalout", () => interactRef.current.hover?.(null, [], { x: 0, y: 0 }));
    return () => {
      ro.disconnect();
      c.dispose();
      chart.current = null;
    };
    // The container remounts this component when the zoom changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const c = chart.current;
    if (!c || !el.current) return;
    const o = visual.objects;
    const colors = palette(el.current);
    const fill = prop<string>(o, def, "colors", "fill");
    const fg = token("--fg-muted", el.current);
    const faint = token("--fg-faint", el.current);
    const border = token("--border", el.current);
    const font = token("--font-ui") || "Inter Variable, sans-serif";
    const vf: FieldFormat = shaped.meta[0]?.format ?? { type: "Double", format: null };
    const lf: FieldFormat = shaped.lineMeta[0]?.format ?? vf;
    const pf: FieldFormat = { type: "Double", format: "0%" };
    const nSeries = shaped.meta.length + shaped.lineMeta.length;
    const showLegend = prop<boolean>(o, def, "legend", "show") && nSeries > 1;
    const legendPos = prop<string>(o, def, "legend", "position");
    const labels = prop<boolean>(o, def, "dataLabels", "show");
    const labelSize = prop<number>(o, def, "dataLabels", "fontSize");
    const catShow = prop<boolean>(o, def, "categoryAxis", "show");
    const valShow = prop<boolean>(o, def, "valueAxis", "show");
    const valGrid = prop<boolean>(o, def, "valueAxis", "gridlines");
    const catGrid = prop<boolean>(o, def, "categoryAxis", "gridlines");
    const catTitle = prop<boolean>(o, def, "categoryAxis", "title") && cat ? catFields.map((p) => projectionName(model, p, t)).join(", ") : undefined;
    const valTitle = prop<boolean>(o, def, "valueAxis", "title") ? shaped.meta.map((s) => s.name).slice(0, 2).join(", ") : undefined;
    const secondary = combo && shaped.lineMeta.length > 0 && prop<boolean>(o, def, "secondaryAxis", "show");
    const secTitle = secondary && prop<boolean>(o, def, "secondaryAxis", "title") ? shaped.lineMeta.map((s) => s.name).slice(0, 2).join(", ") : undefined;
    const seriesColor = (i: number) => (fill && nSeries === 1 ? fill : (colors[i % colors.length] ?? "#888"));
    const lineColor = (j: number) => seriesColor(shaped.meta.length + j);
    const panels = shaped.panels;
    const multiples = !!mult && panels.length > 0;
    const pageTips = pageTipsOn;

    // 100% stacked: each category's values as shares of its total.
    const totals = panels.map((p) => shaped.categories.map((_, i) => p.series.reduce((s, x) => s + Math.abs(x.values[i] ?? 0), 0)));
    const shown = (pi: number, v: number | null, i: number) => (v === null ? null : percent ? (totals[pi]![i] ? v / totals[pi]![i]! : 0) : v);

    // Small multiples share their value axis: its extent over every panel.
    let shared: { min: number; max: number } | null = null;
    if (multiples && !percent && prop<boolean>(o, def, "smallMultiples", "sharedY")) {
      let lo = 0;
      let hi = 0;
      for (const p of panels)
        for (let i = 0; i < shaped.categories.length; i++) {
          if (stacked) {
            let pos = 0;
            let neg = 0;
            for (const s of p.series) {
              const v = s.values[i] ?? 0;
              if (v > 0) pos += v;
              else neg += v;
            }
            hi = Math.max(hi, pos);
            lo = Math.min(lo, neg);
          } else
            for (const s of p.series) {
              const v = s.values[i] ?? 0;
              hi = Math.max(hi, v);
              lo = Math.min(lo, v);
            }
        }
      shared = { min: nice(lo), max: nice(hi) };
    }

    // Grids: one, or a grid of small multiples with a title each.
    const cols = multiples ? Math.max(1, Math.min(panels.length, prop<number>(o, def, "smallMultiples", "columns") || 3)) : 1;
    const rows = Math.ceil(panels.length / cols);
    const top = showLegend && legendPos === "top" ? 32 : 12;
    const bottom = showLegend && legendPos === "bottom" ? 32 : 8;
    // Small multiples: a cell per panel, its title above its grid (in pixels).
    const W = Math.max(1, el.current.offsetWidth);
    const H = Math.max(1, el.current.offsetHeight);
    const cellW = W / cols;
    const cellH = (H - top - bottom) / rows;
    const cell = (pi: number) => ({ x: (pi % cols) * cellW, y: top + Math.floor(pi / cols) * cellH });
    const grids = panels.map((_, pi) => {
      if (!multiples) return { left: 8, right: secondary ? 8 : 16, top, bottom, containLabel: true };
      const { x, y } = cell(pi);
      return { left: x + 4, width: cellW - 12, top: y + 18, height: Math.max(10, cellH - 26), containLabel: true };
    });
    const titles = multiples ? panels.map((p, pi) => ({ text: p.name, left: cell(pi).x + 4, top: cell(pi).y, textStyle: { color: fg, fontSize: 11, fontWeight: 600, fontFamily: font }, padding: 0 })) : [];

    const categoryAxis = (pi: number) => ({
      type: "category" as const,
      gridIndex: pi,
      data: shaped.categories,
      show: catShow,
      inverse: horizontal,
      name: multiples ? undefined : catTitle,
      nameLocation: "middle" as const,
      nameGap: horizontal ? 70 : 28,
      nameTextStyle: { color: faint, fontFamily: font },
      axisLine: { lineStyle: { color: border } },
      axisTick: { show: false },
      splitLine: { show: catGrid, lineStyle: { color: border } },
      axisLabel: { color: fg, fontFamily: font, fontSize: multiples ? 10 : 11, hideOverlap: true, width: horizontal ? (multiples ? 60 : 90) : undefined, overflow: "truncate" as const },
    });
    const valueAxis = (pi: number, f: FieldFormat, name?: string, second = false) => ({
      type: "value" as const,
      gridIndex: pi,
      show: second ? secondary : valShow,
      name: multiples ? undefined : name,
      nameLocation: "middle" as const,
      nameGap: horizontal ? 26 : 46,
      nameTextStyle: { color: faint, fontFamily: font },
      position: second ? ("right" as const) : undefined,
      min: percent ? 0 : shared?.min,
      max: percent ? 1 : shared?.max,
      splitLine: { show: !second && valGrid, lineStyle: { color: border, opacity: 0.6 } },
      axisLabel: { color: fg, fontFamily: font, fontSize: multiples ? 10 : 11, formatter: (v: number) => axisLabel(v, percent ? pf : f, locale), hideOverlap: true },
    });
    const xAxes: object[] = [];
    const yAxes: object[] = [];
    panels.forEach((_, pi) => {
      if (horizontal) {
        xAxes.push(valueAxis(pi, vf, valTitle));
        yAxes.push(categoryAxis(pi));
      } else {
        xAxes.push(categoryAxis(pi));
        yAxes.push(valueAxis(pi, vf, valTitle));
      }
    });
    // A combo chart's secondary axis, for its lines.
    if (secondary) yAxes.push(valueAxis(0, lf, secTitle, true));

    const lineWidth = prop<number>(o, def, "lines", "width");
    const label = (f: FieldFormat) => ({
      show: labels,
      position: stacked ? "inside" : horizontal ? "right" : "top",
      color: stacked ? token("--fg-strong", el.current) : fg,
      fontSize: labelSize,
      formatter: (p: { value: unknown }) => (p.value === null || p.value === undefined ? "" : axisLabel(Number(p.value), percent ? pf : f, locale)),
    });
    // A bar's opacity: faded when another point in this chart is chosen.
    const dim = (category: number, series: number, panel: number) => {
      const pt = point(category, series, panel);
      return pt && interact.chosen(pt.tuple) === false ? 0.35 : 1;
    };
    // Analytics lines, on each series (constant line once per chart).
    const analytics = (si: number, values: (number | null)[], color: string, f: FieldFormat) => {
      if (!def?.objects.some((x) => x.name === "analytics")) return undefined;
      const on = (n: string) => prop<boolean>(o, def, "analytics", n);
      const showLabels = on("labels");
      const lbl = (text: string) => ({ show: showLabels, color: fg, fontSize: 10, formatter: (p: { value: unknown }) => `${text}${text ? ": " : ""}${axisLabel(Number(p.value), f, locale)}`, position: "insideEndTop" as const });
      const axis = horizontal ? "xAxis" : "yAxis";
      const data: object[] = [];
      if (on("average")) data.push({ type: "average", name: "average", label: lbl(t("Average")) });
      if (on("min")) data.push({ type: "min", name: "min", label: lbl(t("Min")) });
      if (on("max")) data.push({ type: "max", name: "max", label: lbl(t("Max")) });
      if (on("trend")) {
        const tr = trend(values);
        if (tr) {
          const end = values.length - 1;
          data.push([
            { name: "trend", coord: horizontal ? [tr[0], 0] : [0, tr[0]], symbol: "none" },
            { coord: horizontal ? [tr[1], end] : [end, tr[1]], symbol: "none", label: { show: false } },
          ]);
        }
      }
      if (on("constant") && si === 0) data.push({ name: "constant", [axis]: prop<number>(o, def, "analytics", "constantValue"), label: lbl(""), lineStyle: { color: token("--fg", el.current), type: "dashed" } });
      if (!data.length) return undefined;
      return { silent: true, symbol: "none", lineStyle: { color, type: "dashed", width: 1.5 }, data, animation: false };
    };
    const axisIndex = (pi: number) => ({ xAxisIndex: pi, yAxisIndex: pi });
    const barSeries = panels.flatMap((panel, pi) =>
      panel.series.flatMap((s, si): Record<string, unknown>[] => {
        const f = shaped.meta[si]?.format ?? vf;
        const color = seriesColor(si);
        const stack = stacked ? `total${pi}` : `s${si}:${pi}`;
        const base = {
          type: "bar" as const,
          name: shaped.meta[si]?.name ?? "",
          barMaxWidth: multiples ? 24 : 48,
          barGap: "12%",
          emphasis: { focus: "series" as const },
          ...axisIndex(pi),
        };
        const values = s.values.map((v, i) => shown(pi, v, i));
        if (!s.hl) {
          return [
            {
              ...base,
              id: `v:${si}:${pi}`,
              stack: stacked ? `total${pi}` : undefined,
              data: values.map((v, i) => ({ value: v, itemStyle: { color, opacity: dim(i, si, pi), borderRadius: stacked ? 0 : horizontal ? [0, 4, 4, 0] : [4, 4, 0, 0] } })),
              label: label(f),
              markLine: percent ? undefined : analytics(si, s.values, color, f),
            },
          ];
        }
        // Highlighted: the highlighted part, then the rest of the bar faded, stacked.
        const hl = s.hl.map((v, i) => shown(pi, v ?? 0, i));
        return [
          { ...base, id: `h:${si}:${pi}`, stack, data: hl.map((v) => ({ value: v ?? 0, itemStyle: { color } })), label: label(f), markLine: percent ? undefined : analytics(si, s.values, color, f) },
          {
            ...base,
            id: `r:${si}:${pi}`,
            stack,
            data: values.map((v, i) => ({ value: v === null ? null : v - (hl[i] ?? 0), itemStyle: { color: fade(color, 0.3) } })),
            label: { show: false },
            tooltip: { show: false },
          },
        ];
      }),
    );
    const lineSeries = panels.flatMap((panel, pi) =>
      panel.series.map((s, si) => ({
        type: "line" as const,
        id: `v:${si}:${pi}`,
        name: shaped.meta[si]?.name ?? "",
        data: s.values,
        color: seriesColor(si),
        showSymbol: prop<boolean>(o, def, "lines", "markers"),
        symbolSize: 7,
        smooth: prop<boolean>(o, def, "lines", "smooth"),
        lineStyle: { width: lineWidth },
        connectNulls: false,
        label: label(shaped.meta[si]?.format ?? vf),
        emphasis: { focus: "series" as const },
        markLine: analytics(si, s.values, seriesColor(si), shaped.meta[si]?.format ?? vf),
        ...axisIndex(pi),
      })),
    );
    // A combo chart's lines, over its columns.
    const comboLines = combo
      ? (panels[0]?.lines ?? []).map((s, j) => ({
          type: "line" as const,
          id: `l:${j}:0`,
          name: shaped.lineMeta[j]?.name ?? "",
          data: s.values,
          color: lineColor(j),
          yAxisIndex: secondary ? 1 : 0,
          showSymbol: prop<boolean>(o, def, "lines", "markers"),
          symbolSize: 6,
          smooth: prop<boolean>(o, def, "lines", "smooth"),
          lineStyle: { width: lineWidth },
          z: 5,
          label: { ...label(shaped.lineMeta[j]?.format ?? lf), position: "top" },
          emphasis: { focus: "series" as const },
        }))
      : [];
    const legendNames = [...shaped.meta.map((s) => s.name), ...shaped.lineMeta.map((s) => s.name)];
    c.setOption(
      {
        animation: document.documentElement.dataset.motion !== "off",
        animationDuration: 250,
        color: legendNames.map((_, i) => seriesColor(i)),
        textStyle: { fontFamily: font },
        title: titles,
        grid: grids,
        legend: {
          show: showLegend,
          type: "scroll",
          orient: legendPos === "right" ? "vertical" : "horizontal",
          [legendPos === "right" ? "right" : legendPos === "bottom" ? "bottom" : "top"]: 0,
          left: legendPos === "right" ? undefined : 0,
          textStyle: { color: fg, fontSize: 11 },
          icon: "circle",
          itemWidth: 8,
          itemHeight: 8,
          pageTextStyle: { color: fg },
          data: legendNames,
        },
        tooltip: {
          trigger: "axis",
          showContent: !pageTips,
          axisPointer: { type: isLine ? "line" : "shadow", shadowStyle: { color: token("--bg-hover", el.current), opacity: 0.5 } },
          backgroundColor: token("--bg-overlay"),
          borderColor: border,
          textStyle: { color: token("--fg"), fontSize: 12, fontFamily: font },
          formatter: (params: unknown) => {
            const ps = (Array.isArray(params) ? params : [params]) as { dataIndex: number; seriesId?: string; marker?: string; axisValueLabel?: string }[];
            const i = ps[0]?.dataIndex ?? 0;
            const pi = Number(ps[0]?.seriesId?.split(":")[2] ?? 0);
            const panel = panels[pi];
            const headText = `${multiples && panel ? `${panel.name} · ` : ""}${ps[0]?.axisValueLabel ?? shaped.categories[i] ?? ""}`;
            const head = `<div style="margin-bottom:4px;font-weight:600">${escape(headText)}</div>`;
            const lines = ps
              .filter((p) => !p.seriesId?.startsWith("r:"))
              .map((p) => {
                const [k, sRaw] = (p.seriesId ?? "").split(":");
                const idx = Number(sRaw ?? 0);
                const isL = k === "l";
                const m = isL ? shaped.lineMeta[idx] : shaped.meta[idx];
                const s = isL ? panel?.lines[idx] : panel?.series[idx];
                if (!m || !s) return "";
                const v = s.values[i];
                const text = v === null || v === undefined ? t("(Blank)") : fmt(v, m.format, locale);
                const share = percent && !isL && v !== null && v !== undefined ? ` <span style="opacity:.7">(${fmt(totals[pi]![i] ? v / totals[pi]![i]! : 0, { type: "Double", format: "0.0%" }, locale)})</span>` : "";
                const hl = s.hl ? s.hl[i] : undefined;
                const part = hl === undefined || isL ? "" : ` <span style="opacity:.7">(${t("highlighted")}: ${hl === null ? t("(Blank)") : fmt(hl, m.format, locale)})</span>`;
                return `<div>${p.marker ?? ""}${escape(m.name)}: <b>${escape(text)}</b>${share}${part}</div>`;
              });
            return head + lines.join("");
          },
        },
        xAxis: xAxes,
        yAxis: yAxes,
        series: isLine ? lineSeries : [...barSeries, ...comboLines],
      },
      { notMerge: true },
    );
    // `point` and `interact` are read through the data they derive from.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shaped, visual.objects, def, kind, model, cat, locale, t, env, themeKey, interact.chosen, pageTipsOn]);

  return <div ref={el} className="cartesian" />;
}

export function escape(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}
