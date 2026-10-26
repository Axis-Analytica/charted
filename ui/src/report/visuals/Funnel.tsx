// The funnel: one centred bar per category, widest first, labelled with its value and
// its share of the first stage. Clicking a stage chooses it.
import { FunnelChart } from "echarts/charts";
import { TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { hierarchy } from "../../state/drill";
import { keysOf, type VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { ThemeKey } from "../scale";
import { escape } from "./Cartesian";
import { axisLabel, fmt, numberAt, palette, projectionFormat } from "./common";
import { chartTheme, useEChart } from "./echart";
import type { Interact } from "./interact";

echarts.use([FunnelChart, TooltipComponent, CanvasRenderer]);

export function Funnel({ visual, model, data, interact }: { visual: Visual; model: Model; data: VisualData; interact: Interact }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const themeKey = useContext(ThemeKey);
  const def = visualDef(visual.visualType);
  const levels = useMemo(() => hierarchy(visual)?.shown ?? [], [visual]);
  const catFields = useMemo(() => levels.map((i) => visual.roles.category![i]!), [levels, visual.roles.category]);
  const val = visual.roles.values?.[0];

  const stages = useMemo(() => {
    const vecs = levels.map((i) => data.columns.get(`category:${i}`));
    const keyCols = levels.map((i) => keysOf(data, `category:${i}`));
    const fmts = catFields.map((p) => projectionFormat(model, p));
    const vv = data.columns.get("values:0");
    return Array.from({ length: Math.min(data.rows, 100) }, (_, r) => ({
      name: vecs.map((v, j) => (v?.get(r) === null || v?.get(r) === undefined ? t("(Blank)") : fmt(v.get(r), fmts[j]!, locale))).join(" ") || (val ? "" : ""),
      keys: keyCols.map((k) => k[r] ?? null),
      value: numberAt(vv, r) ?? 0,
    }));
  }, [data, levels, catFields, model, locale, t, val]);

  const { el, chart } = useEChart(interact, (p) => {
    const s = stages[p.dataIndex];
    return s && catFields.length ? { fields: catFields, tuple: s.keys, drill: s.keys } : null;
  });

  useEffect(() => {
    const c = chart.current;
    if (!c || !el.current) return;
    const th = chartTheme(el.current);
    const o = visual.objects;
    const vf = val ? projectionFormat(model, val) : { type: "Double" as const, format: null };
    const color = prop<string>(o, def, "colors", "fill") || palette(el.current)[0] || "#4e79a7";
    const first = stages[0]?.value || 0;
    const pct = (v: number) => new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 1 }).format(first ? v / first : 0);
    const showPct = prop<boolean>(o, def, "dataLabels", "percent");
    const max = Math.max(0, ...stages.map((s) => s.value));
    c.setOption(
      {
        animation: th.animation,
        animationDuration: 250,
        textStyle: { fontFamily: th.font },
        tooltip: {
          trigger: "item",
          ...th.tooltip,
          formatter: (p: unknown) => {
            const s = stages[(p as { dataIndex: number }).dataIndex];
            return s ? `<div style="font-weight:600">${escape(s.name)}</div><div>${escape(fmt(s.value, vf, locale))} (${t("%1 of first", pct(s.value))})</div>` : "";
          },
        },
        series: [
          {
            type: "funnel",
            id: "stages",
            sort: "none",
            left: 8,
            right: 8,
            top: 8,
            bottom: 8,
            min: 0,
            max: max || 1,
            minSize: "2%",
            gap: 2,
            funnelAlign: "center",
            label: {
              show: prop<boolean>(o, def, "dataLabels", "show"),
              position: "inside",
              color: th.strong,
              fontSize: 11,
              formatter: (p: { dataIndex: number }) => {
                const s = stages[p.dataIndex];
                return s ? `${s.name}: ${axisLabel(s.value, vf, locale)}${showPct ? ` (${pct(s.value)})` : ""}` : "";
              },
            },
            labelLine: { show: false },
            itemStyle: { borderWidth: 0 },
            data: stages.map((s) => ({ name: s.name, value: s.value, itemStyle: { color, opacity: interact.chosen(s.keys) === false ? 0.35 : 1 } })),
          },
        ],
      },
      { notMerge: true },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stages, visual.objects, def, val, model, locale, t, env, themeKey, interact.chosen]);

  return <div ref={el} className="cartesian" />;
}
