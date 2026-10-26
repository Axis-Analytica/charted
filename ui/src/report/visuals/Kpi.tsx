// The KPI: the latest value of an indicator along its trend axis, coloured by how it
// compares with its target (green when on target, red when not, or the other way when
// low is good), with the distance to the target and the trend as an area behind.
import { LineChart } from "echarts/charts";
import { GridComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import type { VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { ThemeKey } from "../scale";
import { fade, fmt, numberAt, projectionFormat, withUnits } from "./common";
import { chartTheme, useEChart } from "./echart";
import { noInteract } from "./interact";
import { kpiState } from "./calc";

echarts.use([LineChart, GridComponent, CanvasRenderer]);


export function Kpi({ visual, model, data }: { visual: Visual; model: Model; data: VisualData }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const themeKey = useContext(ThemeKey);
  const def = visualDef(visual.visualType);
  const o = visual.objects;
  const ind = visual.roles.indicator?.[0];
  const goalField = visual.roles.goal?.[0];
  const hasTrend = !!visual.roles.trend?.length;
  const series = useMemo(() => {
    const vv = data.columns.get("indicator:0");
    const gv = data.columns.get("goal:0");
    const values: (number | null)[] = [];
    const goals: (number | null)[] = [];
    for (let r = 0; r < data.rows; r++) {
      values.push(numberAt(vv, r));
      goals.push(gv ? numberAt(gv, r) : null);
    }
    return { values, goals };
  }, [data]);
  const state = kpiState(series.values, series.goals, prop<string>(o, def, "status", "direction") !== "low");
  const { el, chart } = useEChart(noInteract, () => null);
  const showTrend = hasTrend && prop<boolean>(o, def, "trendline", "show");

  useEffect(() => {
    const c = chart.current;
    if (!c || !el.current) return;
    const th = chartTheme(el.current);
    const color = state.good === null ? th.faint || "#888" : state.good ? th.success : th.danger;
    c.setOption(
      {
        animation: false,
        grid: { left: 0, right: 0, top: 0, bottom: 0 },
        xAxis: { type: "category", show: false, boundaryGap: false, data: series.values.map((_, i) => i) },
        yAxis: { type: "value", show: false, scale: true },
        series: [{ type: "line", id: "trend", data: showTrend ? series.values : [], showSymbol: false, smooth: false, lineStyle: { width: 1.5, color }, areaStyle: { color: fade(color, 0.18) }, silent: true }],
      },
      { notMerge: true },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [series, state.good, showTrend, env, themeKey]);

  const vf = ind ? projectionFormat(model, ind) : { type: "Double" as const, format: null };
  const gf = goalField ? projectionFormat(model, goalField) : vf;
  const units = prop<string>(o, def, "callout", "displayUnits");
  const size = prop<number>(o, def, "callout", "fontSize");
  const distance = prop<string>(o, def, "goals", "distance");
  const pct = state.distance === null ? "" : `${state.distance >= 0 ? "+" : ""}${new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 1 }).format(state.distance)}`;
  const goalText = state.goal === null ? "" : distance === "percent" ? t("Target: %1 (%2)", withUnits(state.goal, units, gf, locale), pct) : distance === "value" ? t("Target: %1", fmt(state.goal, gf, locale)) : t("Target: %1 (%2)", fmt(state.goal, gf, locale), pct);
  return (
    <div className={`kpi-visual ${state.good === null ? "" : state.good ? "is-good" : "is-bad"}`}>
      <div ref={el} className="cartesian kpi-trend" aria-hidden />
      <div className="kpi-body">
        <div className="kpi-value tabular" style={{ fontSize: size }} title={state.value === null ? undefined : fmt(state.value, vf, locale)}>
          {state.value === null ? t("(Blank)") : withUnits(state.value, units, vf, locale)}
        </div>
        {prop<boolean>(o, def, "goals", "show") && goalText && <div className="kpi-goal tabular">{goalText}</div>}
      </div>
    </div>
  );
}
