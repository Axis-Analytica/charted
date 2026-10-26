// The treemap: a rectangle per category sized by its value, split by the details field
// when there is one. Clicking a rectangle chooses its category (and detail).
import { TreemapChart } from "echarts/charts";
import { TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Projection } from "../../bridge/generated/Projection";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { hierarchy } from "../../state/drill";
import { keysOf, type VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { ThemeKey } from "../scale";
import { escape } from "./Cartesian";
import { axisLabel, type FieldFormat, fade, fmt, numberAt, palette, projectionFormat } from "./common";
import { chartTheme, useEChart } from "./echart";
import type { Interact } from "./interact";

echarts.use([TreemapChart, TooltipComponent, CanvasRenderer]);

interface Node {
  name: string;
  /** The group's keys at the levels shown, then the detail's key. */
  keys: (string | null)[];
  value: number;
  children?: Node[];
}

export function Treemap({ visual, model, data, interact }: { visual: Visual; model: Model; data: VisualData; interact: Interact }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const themeKey = useContext(ThemeKey);
  const def = visualDef(visual.visualType);
  const levels = useMemo(() => hierarchy(visual)?.shown ?? [], [visual]);
  const groupFields = useMemo(() => levels.map((i) => visual.roles.group![i]!), [levels, visual.roles.group]);
  const detail = visual.roles.details?.[0];
  const val = visual.roles.values?.[0];

  const nodes = useMemo(() => {
    const vecs = levels.map((i) => data.columns.get(`group:${i}`));
    const keyCols = levels.map((i) => keysOf(data, `group:${i}`));
    const fmts = groupFields.map((p) => projectionFormat(model, p));
    const dVec = data.columns.get("details:0");
    const dKeys = detail ? keysOf(data, "details:0") : [];
    const dFmt = detail ? projectionFormat(model, detail) : null;
    const label = (v: unknown, f: FieldFormat | null) => (v === null || v === undefined ? t("(Blank)") : f ? fmt(v, f, locale) : String(v));
    const vv = data.columns.get("values:0");
    const out: Node[] = [];
    const byGroup = new Map<string, Node>();
    for (let r = 0; r < data.rows; r++) {
      const v = numberAt(vv, r);
      if (v === null || v <= 0) continue;
      const keys = keyCols.map((k) => k[r] ?? null);
      const id = JSON.stringify(keys);
      let g = byGroup.get(id);
      if (!g) {
        g = { name: vecs.map((x, j) => label(x?.get(r), fmts[j] ?? null)).join(" "), keys, value: 0, children: detail ? [] : undefined };
        byGroup.set(id, g);
        out.push(g);
      }
      g.value += v;
      if (detail) g.children!.push({ name: label(dVec?.get(r), dFmt), keys: [...keys, dKeys[r] ?? null], value: v });
    }
    out.sort((a, b) => b.value - a.value);
    return out;
  }, [data, levels, groupFields, detail, model, locale, t]);

  // Data items carry their path: `g` (group index) and `d` (detail index).
  const pointOf = (d: unknown): { fields: Projection[]; tuple: (string | null)[]; drill?: (string | null)[] } | null => {
    const item = d as { g?: number; d?: number } | undefined;
    const g = item?.g === undefined ? undefined : nodes[item.g];
    if (!g || !groupFields.length) return null;
    const n = item?.d === undefined ? undefined : g.children?.[item.d];
    if (n && detail) return { fields: [...groupFields, detail], tuple: n.keys };
    return { fields: groupFields, tuple: g.keys, drill: g.keys };
  };
  const { el, chart } = useEChart(interact, (p) => pointOf(p.data));

  useEffect(() => {
    const c = chart.current;
    if (!c || !el.current) return;
    const th = chartTheme(el.current);
    const o = visual.objects;
    const colors = palette(el.current);
    const vf = val ? projectionFormat(model, val) : { type: "Double" as const, format: null };
    const showLabels = prop<boolean>(o, def, "dataLabels", "show");
    const showValues = prop<boolean>(o, def, "dataLabels", "values");
    const total = nodes.reduce((s, n) => s + n.value, 0);
    const pct = (v: number) => new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 1 }).format(total ? v / total : 0);
    const item = (n: Node, color: string, g: number, d?: number) => {
      const dim = interact.chosen(n.keys) === false || (d !== undefined && interact.chosen(n.keys.slice(0, -1)) === false);
      return {
        name: n.name,
        value: n.value,
        g,
        d,
        itemStyle: { color: dim ? fade(color, 0.35) : color, borderColor: th.bg, borderWidth: 1, gapWidth: 1 },
      };
    };
    c.setOption(
      {
        animation: th.animation,
        animationDuration: 250,
        textStyle: { fontFamily: th.font },
        tooltip: {
          trigger: "item",
          ...th.tooltip,
          formatter: (p: unknown) => {
            const d = (p as { data?: { name: string; value: number } }).data;
            return d ? `<div style="font-weight:600">${escape(d.name)}</div><div>${escape(fmt(d.value, vf, locale))} (${pct(d.value)})</div>` : "";
          },
        },
        series: [
          {
            type: "treemap",
            id: "tree",
            left: 0,
            right: 0,
            top: 0,
            bottom: 0,
            roam: false,
            nodeClick: false,
            breadcrumb: { show: false },
            squareRatio: 0.5 * (1 + Math.sqrt(5)),
            label: {
              show: showLabels,
              color: th.strong,
              fontSize: 11,
              overflow: "truncate",
              formatter: (p: { data: { name: string; value: number } }) => (showValues ? `${p.data.name}\n${axisLabel(p.data.value, vf, locale)}` : p.data.name),
            },
            upperLabel: detail ? { show: showLabels, height: 18, color: th.strong, fontSize: 11 } : undefined,
            levels: detail ? [{ itemStyle: { gapWidth: 2, borderColor: th.bg } }, { itemStyle: { gapWidth: 1 } }] : undefined,
            data: nodes.map((n, g) => {
              const color = colors[g % colors.length] ?? "#888";
              return { ...item(n, color, g), children: n.children?.map((ch, d) => item(ch, color, g, d)) };
            }),
          },
        ],
      },
      { notMerge: true },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodes, visual.objects, def, val, model, locale, t, env, themeKey, interact.chosen]);

  return <div ref={el} className="cartesian" />;
}
