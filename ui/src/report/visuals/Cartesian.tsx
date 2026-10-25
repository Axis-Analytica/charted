// Clustered column, clustered bar and line charts (ECharts, canvas). Categories come
// from the category role; series from the legend role (one value pivoted by it) or
// from each value field.
import { BarChart, LineChart } from "echarts/charts";
import { GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo, useRef } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { projectionName } from "../../state/report";
import type { VisualData } from "../data";
import { CanvasScale } from "../scale";
import { prop, visualDef } from "../registry";
import { axisLabel, fmt, palette, projectionFormat, token } from "./common";

echarts.use([BarChart, LineChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer]);

/** Series beyond this many (by legend value) are left out, as Power BI's top 60. */
const MAX_SERIES = 60;

interface Series {
  name: string;
  values: (number | null)[];
  /** Which value projection the series shows (for formatting). */
  value: number;
}

export function Cartesian({ visual, model, data }: { visual: Visual; model: Model; data: VisualData }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const scale = useContext(CanvasScale);
  const def = visualDef(visual.visualType);
  const kind = visual.visualType;
  const cat = visual.roles.category?.[0];
  const ser = visual.roles.series?.[0];
  const vals = visual.roles.values ?? [];

  const shaped = useMemo(() => {
    const catVec = data.columns.get("category:0");
    const serVec = data.columns.get("series:0");
    const catFmt = cat ? projectionFormat(model, cat) : null;
    const serFmt = ser ? projectionFormat(model, ser) : null;
    const label = (v: unknown, f: ReturnType<typeof projectionFormat> | null) => (v === null || v === undefined ? t("(Blank)") : f ? fmt(v, f, locale) : String(v));
    const categories: string[] = [];
    const catIndex = new Map<string, number>();
    const series: Series[] = [];
    if (ser && serVec && vals.length) {
      // Pivot the first value by the legend field.
      const byName = new Map<string, Series>();
      const valVec = data.columns.get("values:0");
      for (let r = 0; r < data.rows; r++) {
        const c = cat ? label(catVec?.get(r), catFmt) : "";
        if (!catIndex.has(c)) {
          catIndex.set(c, categories.length);
          categories.push(c);
        }
        const s = label(serVec.get(r), serFmt);
        let entry = byName.get(s);
        if (!entry) {
          if (byName.size >= MAX_SERIES) continue;
          entry = { name: s, values: [], value: 0 };
          byName.set(s, entry);
          series.push(entry);
        }
        const v = valVec?.get(r);
        entry.values[catIndex.get(c)!] = v === null || v === undefined ? null : Number(v);
      }
      for (const s of series) for (let i = 0; i < categories.length; i++) s.values[i] ??= null;
    } else {
      for (let r = 0; r < data.rows; r++) categories.push(cat ? label(catVec?.get(r), catFmt) : "");
      vals.forEach((p, i) => {
        const vec = data.columns.get(`values:${i}`);
        const values: (number | null)[] = [];
        for (let r = 0; r < data.rows; r++) {
          const v = vec?.get(r);
          values.push(v === null || v === undefined ? null : Number(v));
        }
        series.push({ name: projectionName(model, p, t), values, value: i });
      });
    }
    return { categories, series };
  }, [data, cat, ser, vals, model, locale, t]);

  useEffect(() => {
    if (!el.current) return;
    // Under a scaled canvas, draw at enough pixels for the zoom.
    const c = echarts.init(el.current, undefined, { renderer: "canvas", devicePixelRatio: (window.devicePixelRatio || 1) * Math.max(1, scale) });
    chart.current = c;
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(el.current);
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
    if (!c) return;
    const o = visual.objects;
    const colors = palette();
    const fill = prop<string>(o, def, "colors", "fill");
    const fg = token("--fg-muted");
    const faint = token("--fg-faint");
    const border = token("--border");
    const font = token("--font-ui") || "Inter Variable, sans-serif";
    const horizontal = kind === "barChart";
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
    c.setOption(
      {
        animation: document.documentElement.dataset.motion !== "off",
        animationDuration: 250,
        color: fill && shaped.series.length === 1 ? [fill] : colors,
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
        },
        tooltip: {
          trigger: "axis",
          axisPointer: { type: kind === "lineChart" ? "line" : "shadow", shadowStyle: { color: token("--bg-hover"), opacity: 0.5 } },
          backgroundColor: token("--bg-overlay"),
          borderColor: border,
          textStyle: { color: token("--fg"), fontSize: 12, fontFamily: font },
          valueFormatter: (v: unknown) => (v === null || v === undefined ? t("(Blank)") : fmt(v, vf, locale)),
        },
        xAxis: horizontal ? valueAxis : categoryAxis,
        yAxis: horizontal ? categoryAxis : valueAxis,
        series: shaped.series.map((s) => {
          const f = valueFormats[s.value] ?? vf;
          const common = {
            name: s.name,
            data: s.values,
            label: {
              show: labels,
              position: horizontal ? "right" : "top",
              color: fg,
              fontSize: labelSize,
              formatter: (p: { value: unknown }) => (p.value === null ? "" : axisLabel(Number(p.value), f, locale)),
            },
            emphasis: { focus: "series" as const },
          };
          return kind === "lineChart"
            ? {
                ...common,
                type: "line" as const,
                showSymbol: prop<boolean>(o, def, "lines", "markers"),
                symbolSize: 7,
                smooth: prop<boolean>(o, def, "lines", "smooth"),
                lineStyle: { width: lineWidth },
                connectNulls: false,
              }
            : {
                ...common,
                type: "bar" as const,
                barMaxWidth: 48,
                barGap: "12%",
                itemStyle: { borderRadius: horizontal ? [0, 4, 4, 0] : [4, 4, 0, 0] },
              };
        }),
      },
      { notMerge: true },
    );
  }, [shaped, visual.objects, def, kind, vals, model, cat, locale, t, env]);

  return <div ref={el} className="cartesian" />;
}
