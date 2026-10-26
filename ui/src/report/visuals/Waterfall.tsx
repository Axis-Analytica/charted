// The waterfall chart: each category's value as a step up (increase) or down
// (decrease) from the running total, ending in a total bar. With a breakdown field, the
// first category is a total and each next one shows how each breakdown member changed
// from the category before (the largest few, the rest as Other), then its total.
import { BarChart, CustomChart } from "echarts/charts";
import { GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Projection } from "../../bridge/generated/Projection";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { hierarchy } from "../../state/drill";
import { projectionName } from "../../state/report";
import { keysOf, type VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { ThemeKey } from "../scale";
import { escape } from "./Cartesian";
import { axisLabel, type FieldFormat, fmt, numberAt, projectionFormat } from "./common";
import { chartTheme, useEChart } from "./echart";
import type { Interact } from "./interact";
import { type StepKind, waterfallSteps } from "./calc";

echarts.use([BarChart, CustomChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer]);


export function Waterfall({ visual, model, data, interact }: { visual: Visual; model: Model; data: VisualData; interact: Interact }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const themeKey = useContext(ThemeKey);
  const def = visualDef(visual.visualType);
  const levels = useMemo(() => hierarchy(visual)?.shown ?? [], [visual]);
  const catFields = useMemo(() => levels.map((i) => visual.roles.category![i]!), [levels, visual.roles.category]);
  const brk = visual.roles.breakdown?.[0];
  const val = visual.roles.values?.[0];
  const o = visual.objects;

  const steps = useMemo(() => {
    const catVecs = levels.map((i) => data.columns.get(`category:${i}`));
    const catKeys = levels.map((i) => keysOf(data, `category:${i}`));
    const catFmts = catFields.map((p) => projectionFormat(model, p));
    const bVec = data.columns.get("breakdown:0");
    const bKeys = brk ? keysOf(data, "breakdown:0") : [];
    const bFmt = brk ? projectionFormat(model, brk) : null;
    const label = (v: unknown, f: FieldFormat | null) => (v === null || v === undefined ? t("(Blank)") : f ? fmt(v, f, locale) : String(v));
    const vv = data.columns.get("values:0");
    const rows = Array.from({ length: data.rows }, (_, r) => ({
      label: catVecs.map((v, j) => label(v?.get(r), catFmts[j] ?? null)).join(" "),
      keys: catKeys.map((k) => k[r] ?? null),
      breakdown: brk ? { key: bKeys[r] ?? null, label: label(bVec?.get(r), bFmt) } : null,
      value: numberAt(vv, r),
    }));
    return waterfallSteps(rows, { total: prop<boolean>(o, def, "breakdown", "total"), maxBreakdowns: prop<number>(o, def, "breakdown", "max"), totalLabel: t("Total"), otherLabel: t("Other") });
  }, [data, levels, catFields, brk, model, locale, t, o, def]);

  const point = (i: number): { fields: Projection[]; tuple: (string | null)[]; drill?: (string | null)[] } | null => {
    const s = steps[i];
    if (!s || !s.keys.length) return null;
    if (s.breakdown !== undefined && brk) return { fields: [...catFields, brk], tuple: [...s.keys, s.breakdown] };
    return { fields: catFields, tuple: s.keys, drill: s.keys };
  };
  const { el, chart } = useEChart(interact, (p) => point(p.dataIndex));

  useEffect(() => {
    const c = chart.current;
    if (!c || !el.current) return;
    const th = chartTheme(el.current);
    const vf = val ? projectionFormat(model, val) : { type: "Double" as const, format: null };
    const color = (k: StepKind) => prop<string>(o, def, "sentimentColors", k) || (k === "increase" ? th.success : k === "decrease" ? th.danger : "");
    const totalColor = color("total") || th.faint || "#888";
    const fill = (k: StepKind) => (k === "total" ? totalColor : color(k));
    const showLegend = prop<boolean>(o, def, "legend", "show");
    const legendPos = prop<string>(o, def, "legend", "position");
    const labels = prop<boolean>(o, def, "dataLabels", "show");
    const dims = steps.map((s, i) => [i, s.start, s.end, ["increase", "decrease", "total"].indexOf(s.kind)]);
    const dimmed = (i: number) => {
      const pt = point(i);
      return pt && interact.chosen(pt.tuple) === false;
    };
    c.setOption(
      {
        animation: th.animation,
        animationDuration: 250,
        textStyle: { fontFamily: th.font },
        grid: { left: 8, right: 16, top: showLegend && legendPos === "top" ? 32 : 16, bottom: showLegend && legendPos === "bottom" ? 32 : 8, containLabel: true },
        legend: {
          show: showLegend,
          [legendPos === "right" ? "right" : legendPos === "bottom" ? "bottom" : "top"]: 0,
          orient: legendPos === "right" ? "vertical" : "horizontal",
          left: legendPos === "right" ? undefined : 0,
          textStyle: { color: th.fg, fontSize: 11 },
          icon: "circle",
          itemWidth: 8,
          itemHeight: 8,
          data: [t("Increase"), t("Decrease"), t("Total")],
        },
        tooltip: {
          trigger: "item",
          ...th.tooltip,
          formatter: (p: unknown) => {
            const s = steps[(p as { dataIndex: number }).dataIndex];
            if (!s) return "";
            const d = s.end - s.start;
            const kindLabel = s.kind === "total" ? t("Total") : s.kind === "increase" ? t("Increase") : t("Decrease");
            return `<div style="font-weight:600">${escape(s.label)}</div><div>${escape(kindLabel)}: <b>${escape(fmt(s.kind === "total" ? s.end : d, vf, locale))}</b></div>`;
          },
        },
        xAxis: {
          type: "category",
          data: steps.map((s) => s.label),
          show: prop<boolean>(o, def, "categoryAxis", "show"),
          axisLine: { lineStyle: { color: th.border } },
          axisTick: { show: false },
          axisLabel: { color: th.fg, fontSize: 11, hideOverlap: true, overflow: "truncate", width: 90 },
          name: prop<boolean>(o, def, "categoryAxis", "title") ? catFields.map((p) => projectionName(model, p, t)).join(", ") : undefined,
          nameLocation: "middle",
          nameGap: 28,
        },
        yAxis: {
          type: "value",
          show: prop<boolean>(o, def, "valueAxis", "show"),
          splitLine: { show: prop<boolean>(o, def, "valueAxis", "gridlines"), lineStyle: { color: th.border, opacity: 0.6 } },
          axisLabel: { color: th.fg, fontSize: 11, formatter: (v: number) => axisLabel(v, vf, locale) },
          name: prop<boolean>(o, def, "valueAxis", "title") && val ? projectionName(model, val, t) : undefined,
          nameLocation: "middle",
          nameGap: 46,
        },
        series: [
          {
            type: "custom",
            id: "steps",
            encode: { x: 0, y: [1, 2] },
            data: dims,
            renderItem: (_params: unknown, api: { value(i: number): number; coord(v: number[]): number[]; size(v: number[]): number[]; style(): object }) => {
              const i = api.value(0);
              const a = api.coord([i, api.value(1)]);
              const b = api.coord([i, api.value(2)]);
              const w = api.size([1, 0])[0]! * 0.62;
              const s = steps[i]!;
              return {
                type: "rect",
                shape: { x: a[0]! - w / 2, y: Math.min(a[1]!, b[1]!), width: w, height: Math.max(1, Math.abs(a[1]! - b[1]!)) },
                style: { fill: fill(s.kind), opacity: dimmed(i) ? 0.35 : 1 },
                textContent: labels ? { style: { text: axisLabel(s.kind === "total" ? s.end : s.end - s.start, vf, locale), fill: th.fg, fontSize: prop<number>(o, def, "dataLabels", "fontSize"), fontFamily: th.font } } : undefined,
                textConfig: labels ? { position: s.end >= s.start ? "top" : "bottom" } : undefined,
              };
            },
          },
          // Legend entries for the colours.
          ...(["increase", "decrease", "total"] as StepKind[]).map((k) => ({ type: "bar", name: k === "total" ? t("Total") : k === "increase" ? t("Increase") : t("Decrease"), data: [], itemStyle: { color: fill(k) } })),
        ],
      },
      { notMerge: true },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [steps, o, def, val, model, locale, t, env, themeKey, interact.chosen]);

  return <div ref={el} className="cartesian" />;
}
