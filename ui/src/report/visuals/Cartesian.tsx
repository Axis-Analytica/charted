// Clustered and stacked column and bar charts, and line charts (ECharts, canvas).
// Categories come from the category role; series from the legend role (one value
// pivoted by it) or from each value field. Clicking a bar or a point on a line chooses
// it (Ctrl adds to the choice); a highlight from elsewhere draws each bar's
// highlighted part solid and the rest faded.
import { BarChart, LineChart } from "echarts/charts";
import { GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo, useRef } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Projection } from "../../bridge/generated/Projection";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { projectionName } from "../../state/report";
import { keysOf, type VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { CanvasScale, ThemeKey } from "../scale";
import { axisLabel, fade, fmt, numberAt, palette, projectionFormat, token } from "./common";
import { additive, type Interact } from "./interact";

echarts.use([BarChart, LineChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer]);

/** Series beyond this many (by legend value) are left out, as Power BI's top 60. */
const MAX_SERIES = 60;

interface Series {
  name: string;
  /** The legend value's key, when series come from the legend role. */
  key: string | null;
  values: (number | null)[];
  /** Highlighted parts, when a selection elsewhere highlights this chart. */
  hl: (number | null)[] | null;
  /** Which value projection the series shows (for formatting). */
  value: number;
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
  const cat = visual.roles.category?.[0];
  const ser = visual.roles.series?.[0];
  const vals = visual.roles.values ?? [];
  const stacked = kind === "stackedColumnChart" || kind === "stackedBarChart";
  const horizontal = kind === "barChart" || kind === "stackedBarChart";
  const isLine = kind === "lineChart";

  const shaped = useMemo(() => {
    const catVec = data.columns.get("category:0");
    const serVec = data.columns.get("series:0");
    const catKeys = cat ? keysOf(data, "category:0") : [];
    const serKeys = ser ? keysOf(data, "series:0") : [];
    const catFmt = cat ? projectionFormat(model, cat) : null;
    const serFmt = ser ? projectionFormat(model, ser) : null;
    const label = (v: unknown, f: ReturnType<typeof projectionFormat> | null) => (v === null || v === undefined ? t("(Blank)") : f ? fmt(v, f, locale) : String(v));
    const categories: string[] = [];
    const keys: (string | null)[] = [];
    const catIndex = new Map<string, number>();
    const series: Series[] = [];
    if (ser && serVec && vals.length) {
      // Pivot the first value by the legend field.
      const byKey = new Map<string, Series>();
      const valVec = data.columns.get("values:0");
      const hlVec = data.columns.get("hl:values:0");
      for (let r = 0; r < data.rows; r++) {
        const ck = cat ? (catKeys[r] ?? null) : null;
        const cid = String(ck);
        if (!catIndex.has(cid)) {
          catIndex.set(cid, categories.length);
          categories.push(cat ? label(catVec?.get(r), catFmt) : "");
          keys.push(ck);
        }
        const sk = serKeys[r] ?? null;
        let entry = byKey.get(String(sk));
        if (!entry) {
          if (byKey.size >= MAX_SERIES) continue;
          entry = { name: label(serVec.get(r), serFmt), key: sk, values: [], hl: hlVec ? [] : null, value: 0 };
          byKey.set(String(sk), entry);
          series.push(entry);
        }
        const i = catIndex.get(cid)!;
        entry.values[i] = numberAt(valVec, r);
        if (entry.hl) entry.hl[i] = numberAt(hlVec, r);
      }
      for (const s of series)
        for (let i = 0; i < categories.length; i++) {
          s.values[i] ??= null;
          if (s.hl) s.hl[i] ??= null;
        }
    } else {
      for (let r = 0; r < data.rows; r++) {
        categories.push(cat ? label(catVec?.get(r), catFmt) : "");
        keys.push(cat ? (catKeys[r] ?? null) : null);
      }
      vals.forEach((p, i) => {
        const vec = data.columns.get(`values:${i}`);
        const hlVec = data.columns.get(`hl:values:${i}`);
        const values: (number | null)[] = [];
        const hl: (number | null)[] | null = hlVec ? [] : null;
        for (let r = 0; r < data.rows; r++) {
          values.push(numberAt(vec, r));
          hl?.push(numberAt(hlVec, r));
        }
        series.push({ name: projectionName(model, p, t), key: null, values, hl, value: i });
      });
    }
    return { categories, keys, series };
  }, [data, cat, ser, vals, model, locale, t]);

  // The fields and values that identify a point.
  const point = (category: number, series: number | null): { fields: Projection[]; tuple: (string | null)[] } | null => {
    const fields: Projection[] = [];
    const tuple: (string | null)[] = [];
    if (cat) {
      fields.push(cat);
      tuple.push(shaped.keys[category] ?? null);
    }
    const s = series === null ? null : shaped.series[series];
    if (ser && s) {
      fields.push(ser);
      tuple.push(s.key);
    }
    return fields.length ? { fields, tuple } : null;
  };
  const pointRef = useRef(point);
  pointRef.current = point;
  const interactRef = useRef(interact);
  interactRef.current = interact;

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
    c.on("click", (p) => {
      onItem = true;
      const params = p as unknown as { dataIndex: number; seriesIndex: number; event?: { event?: MouseEvent } };
      // Highlighted bars are drawn as pairs of series; both stand for the same one.
      const s = (c.getOption() as { series?: { id?: string }[] }).series?.[params.seriesIndex]?.id ?? "";
      const series = Number(s.split(":")[1] ?? 0);
      const pt = pointRef.current(params.dataIndex, series);
      if (pt) interactRef.current.choose(pt.fields, pt.tuple, additive(params.event?.event));
    });
    c.getZr().on("click", (e) => {
      if (onItem) {
        onItem = false;
        return;
      }
      // Lines: the category under the pointer. Elsewhere: empty space clears.
      const opt = c.getOption() as { series?: { type?: string }[] };
      if (opt.series?.[0]?.type === "line") {
        const at = c.convertFromPixel({ seriesIndex: 0 }, [e.offsetX, e.offsetY]) as number[] | undefined;
        const i = at ? Math.round(at[0] ?? -1) : -1;
        if (c.containPixel({ gridIndex: 0 }, [e.offsetX, e.offsetY]) && i >= 0) {
          const pt = pointRef.current(i, null);
          if (pt) interactRef.current.choose(pt.fields, pt.tuple, additive(e.event as unknown as MouseEvent));
          return;
        }
      }
      interactRef.current.clear();
    });
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
    const valueFormats = vals.map((p) => projectionFormat(model, p));
    const vf = valueFormats[0] ?? { type: "Double" as const, format: null };
    const showLegend = prop<boolean>(o, def, "legend", "show") && shaped.series.length > 1;
    const legendPos = prop<string>(o, def, "legend", "position");
    const labels = prop<boolean>(o, def, "dataLabels", "show");
    const labelSize = prop<number>(o, def, "dataLabels", "fontSize");
    const catShow = prop<boolean>(o, def, "categoryAxis", "show");
    const valShow = prop<boolean>(o, def, "valueAxis", "show");
    const valGrid = prop<boolean>(o, def, "valueAxis", "gridlines");
    const catGrid = prop<boolean>(o, def, "categoryAxis", "gridlines");
    const catTitle = prop<boolean>(o, def, "categoryAxis", "title") && cat ? projectionName(model, cat, t) : undefined;
    const valTitle = prop<boolean>(o, def, "valueAxis", "title") ? shaped.series.map((s) => s.name).slice(0, 2).join(", ") : undefined;
    const seriesColor = (i: number) => (fill && shaped.series.length === 1 ? fill : (colors[i % colors.length] ?? "#888"));
    const categoryAxis = {
      type: "category" as const,
      data: shaped.categories,
      show: catShow,
      inverse: horizontal,
      name: catTitle,
      nameLocation: "middle" as const,
      nameGap: horizontal ? 70 : 28,
      nameTextStyle: { color: faint, fontFamily: font },
      axisLine: { lineStyle: { color: border } },
      axisTick: { show: false },
      splitLine: { show: catGrid, lineStyle: { color: border } },
      axisLabel: { color: fg, fontFamily: font, fontSize: 11, hideOverlap: true, width: horizontal ? 90 : undefined, overflow: "truncate" as const },
    };
    const valueAxis = {
      type: "value" as const,
      show: valShow,
      name: valTitle,
      nameLocation: "middle" as const,
      nameGap: horizontal ? 26 : 46,
      nameTextStyle: { color: faint, fontFamily: font },
      splitLine: { show: valGrid, lineStyle: { color: border, opacity: 0.6 } },
      axisLabel: { color: fg, fontFamily: font, fontSize: 11, formatter: (v: number) => axisLabel(v, vf, locale) },
    };
    const lineWidth = prop<number>(o, def, "lines", "width");
    const label = (f: typeof vf) => ({
      show: labels,
      position: stacked ? "inside" : horizontal ? "right" : "top",
      color: stacked ? token("--fg-strong", el.current) : fg,
      fontSize: labelSize,
      formatter: (p: { value: unknown }) => (p.value === null || p.value === undefined ? "" : axisLabel(Number(p.value), f, locale)),
    });
    // A bar's opacity: faded when another point in this chart is chosen.
    const dim = (category: number, series: number) => {
      const pt = point(category, series);
      return pt && interact.chosen(pt.tuple) === false ? 0.35 : 1;
    };
    const barSeries = shaped.series.flatMap((s, si): Record<string, unknown>[] => {
      const f = valueFormats[s.value] ?? vf;
      const color = seriesColor(si);
      const stack = stacked ? "total" : `s${si}`;
      const base = {
        type: "bar" as const,
        name: s.name,
        barMaxWidth: 48,
        barGap: "12%",
        emphasis: { focus: "series" as const },
      };
      if (!s.hl) {
        return [
          {
            ...base,
            id: `v:${si}`,
            stack: stacked ? "total" : undefined,
            data: s.values.map((v, i) => ({ value: v, itemStyle: { color, opacity: dim(i, si), borderRadius: stacked ? 0 : horizontal ? [0, 4, 4, 0] : [4, 4, 0, 0] } })),
            label: label(f),
          },
        ];
      }
      // Highlighted: the highlighted part, then the rest of the bar faded, stacked.
      return [
        { ...base, id: `h:${si}`, stack, data: s.hl.map((v) => ({ value: v ?? 0, itemStyle: { color } })), label: label(f) },
        {
          ...base,
          id: `r:${si}`,
          stack,
          data: s.values.map((v, i) => ({ value: v === null ? null : v - (s.hl?.[i] ?? 0), itemStyle: { color: fade(color, 0.3) } })),
          label: { show: false },
          tooltip: { show: false },
        },
      ];
    });
    const lineSeries = shaped.series.map((s, si) => ({
      type: "line" as const,
      id: `v:${si}`,
      name: s.name,
      data: s.values,
      color: seriesColor(si),
      showSymbol: prop<boolean>(o, def, "lines", "markers"),
      symbolSize: 7,
      smooth: prop<boolean>(o, def, "lines", "smooth"),
      lineStyle: { width: lineWidth },
      connectNulls: false,
      label: label(valueFormats[s.value] ?? vf),
      emphasis: { focus: "series" as const },
    }));
    c.setOption(
      {
        animation: document.documentElement.dataset.motion !== "off",
        animationDuration: 250,
        color: shaped.series.map((_, i) => seriesColor(i)),
        textStyle: { fontFamily: font },
        grid: { left: 8, right: 16, top: showLegend && legendPos === "top" ? 32 : 12, bottom: showLegend && legendPos === "bottom" ? 32 : 8, containLabel: true },
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
          data: shaped.series.map((s) => s.name),
        },
        tooltip: {
          trigger: "axis",
          axisPointer: { type: isLine ? "line" : "shadow", shadowStyle: { color: token("--bg-hover", el.current), opacity: 0.5 } },
          backgroundColor: token("--bg-overlay"),
          borderColor: border,
          textStyle: { color: token("--fg"), fontSize: 12, fontFamily: font },
          formatter: (params: unknown) => {
            const ps = (Array.isArray(params) ? params : [params]) as { dataIndex: number; seriesId?: string; marker?: string; axisValueLabel?: string }[];
            const i = ps[0]?.dataIndex ?? 0;
            const head = `<div style="margin-bottom:4px;font-weight:600">${escape(ps[0]?.axisValueLabel ?? shaped.categories[i] ?? "")}</div>`;
            const lines = ps
              .filter((p) => !p.seriesId?.startsWith("r:"))
              .map((p) => {
                const si = Number(p.seriesId?.split(":")[1] ?? 0);
                const s = shaped.series[si];
                if (!s) return "";
                const f = valueFormats[s.value] ?? vf;
                const v = s.values[i];
                const text = v === null || v === undefined ? t("(Blank)") : fmt(v, f, locale);
                const hl = s.hl ? s.hl[i] : undefined;
                const part = hl === undefined ? "" : ` <span style="opacity:.7">(${t("highlighted")}: ${hl === null ? t("(Blank)") : fmt(hl, f, locale)})</span>`;
                return `<div>${p.marker ?? ""}${escape(s.name)}: <b>${escape(text)}</b>${part}</div>`;
              });
            return head + lines.join("");
          },
        },
        xAxis: horizontal ? valueAxis : categoryAxis,
        yAxis: horizontal ? categoryAxis : valueAxis,
        series: isLine ? lineSeries : barSeries,
      },
      { notMerge: true },
    );
    // `point` and `interact` are read through the data they derive from.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shaped, visual.objects, def, kind, vals, model, cat, locale, t, env, themeKey, interact.chosen]);

  return <div ref={el} className="cartesian" />;
}

export function escape(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}
