// The gauge: a value on a half-circle arc from a minimum to a maximum, with an optional
// target marker. Minimum, maximum and target come from fields or from the format pane;
// without a maximum the arc runs to twice the value, as in Power BI.
import { GaugeChart } from "echarts/charts";
import { TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { projectionName } from "../../state/report";
import type { VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { ThemeKey } from "../scale";
import { escape } from "./Cartesian";
import { axisLabel, fmt, numberAt, palette, projectionFormat, withUnits } from "./common";
import { chartTheme, useEChart } from "./echart";
import { noInteract } from "./interact";
import { gaugeRange } from "./calc";

echarts.use([GaugeChart, TooltipComponent, CanvasRenderer]);


export function Gauge({ visual, model, data }: { visual: Visual; model: Model; data: VisualData }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const themeKey = useContext(ThemeKey);
  const def = visualDef(visual.visualType);
  const val = visual.roles.values?.[0];
  const o = visual.objects;
  const g = useMemo(() => {
    const at = (role: string) => (visual.roles[role]?.length ? numberAt(data.columns.get(`${role}:0`), 0) : null);
    const value = at("values");
    const range = gaugeRange(value, { min: at("minValue"), max: at("maxValue"), target: at("targetValue") }, { min: prop<string>(o, def, "gaugeAxis", "min"), max: prop<string>(o, def, "gaugeAxis", "max"), target: prop<string>(o, def, "gaugeAxis", "target") });
    return { value, ...range };
  }, [data, visual.roles, o, def]);
  const { el, chart } = useEChart(noInteract, () => null);

  useEffect(() => {
    const c = chart.current;
    if (!c || !el.current) return;
    const th = chartTheme(el.current);
    const vf = val ? projectionFormat(model, val) : { type: "Double" as const, format: null };
    const fill = prop<string>(o, def, "colors", "fill") || palette(el.current)[0] || "#4e79a7";
    const targetColor = prop<string>(o, def, "colors", "target") || th.strong;
    const units = prop<string>(o, def, "callout", "displayUnits");
    const size = prop<number>(o, def, "callout", "fontSize");
    const text = g.value === null ? t("(Blank)") : withUnits(g.value, units, vf, locale);
    const arc = { startAngle: 180, endAngle: 0, min: g.min, max: g.max, center: ["50%", "72%"], radius: "92%" };
    const series: object[] = [
      {
        type: "gauge",
        id: "value",
        ...arc,
        progress: { show: true, width: 22, itemStyle: { color: fill } },
        axisLine: { lineStyle: { width: 22, color: [[1, th.border || "#ddd"]] } },
        pointer: { show: false },
        axisTick: { show: false },
        splitLine: { show: false },
        axisLabel: { show: true, distance: -12, color: th.fg, fontSize: 10, formatter: (v: number) => (v === g.min || v === g.max ? axisLabel(v, vf, locale) : "") },
        splitNumber: 1,
        anchor: { show: false },
        title: { show: false },
        detail: { valueAnimation: false, offsetCenter: [0, "-12%"], fontSize: size, fontWeight: 600, color: th.strong, fontFamily: th.font, formatter: () => text },
        data: [{ value: g.value ?? g.min, name: val ? projectionName(model, val, t) : "" }],
      },
    ];
    // The target: a thin band across the arc at its position.
    if (g.target !== null) {
      const at = Math.min(1, Math.max(0, (g.target - g.min) / (g.max - g.min || 1)));
      const half = 0.006;
      series.push({
        type: "gauge",
        id: "target",
        ...arc,
        z: 3,
        silent: true,
        axisLine: { lineStyle: { width: 30, color: [[Math.max(0, at - half), "rgba(0,0,0,0)"], [Math.min(1, at + half), targetColor], [1, "rgba(0,0,0,0)"]] } },
        progress: { show: false },
        axisTick: { show: false },
        splitLine: { show: false },
        axisLabel: { show: false },
        anchor: { show: false },
        title: { show: false },
        detail: { show: false },
        pointer: { show: false },
        data: [{ value: g.target, name: t("Target") }],
      });
    }
    c.setOption(
      {
        animation: th.animation,
        animationDuration: 250,
        textStyle: { fontFamily: th.font },
        tooltip: {
          trigger: "item",
          ...th.tooltip,
          formatter: () => {
            const rows = [[val ? projectionName(model, val, t) : t("Value"), g.value], [t("Minimum value"), g.min], [t("Maximum value"), g.max], ...(g.target !== null ? [[t("Target value"), g.target]] : [])] as [string, number | null][];
            return rows.map(([k, v]) => `<div>${escape(k)}: <b>${v === null ? t("(Blank)") : escape(fmt(v, vf, locale))}</b></div>`).join("");
          },
        },
        series,
      },
      { notMerge: true },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [g, o, def, val, model, locale, t, env, themeKey]);

  return <div ref={el} className="cartesian gauge-visual" />;
}
