// Pie and donut charts (ECharts). Slices come from the legend role and are sized by
// the first value. Clicking a slice chooses it; a highlight from elsewhere fills each
// slice's highlighted share solid and leaves the rest faded.
import { PieChart } from "echarts/charts";
import { LegendComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useContext, useEffect, useMemo, useRef } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { keysOf, type VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { CanvasScale, ThemeKey } from "../scale";
import { escape } from "./Cartesian";
import { fade, fmt, numberAt, palette, projectionFormat, token } from "./common";
import { additive, type Interact } from "./interact";

echarts.use([PieChart, TooltipComponent, LegendComponent, CanvasRenderer]);

const MAX_SLICES = 60;

export function Pie({ visual, model, data, interact }: { visual: Visual; model: Model; data: VisualData; interact: Interact }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const themeKey = useContext(ThemeKey);
  const scale = useContext(CanvasScale);
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const def = visualDef(visual.visualType);
  const leg = visual.roles.legend?.[0];
  const val = visual.roles.values?.[0];
  const donut = visual.visualType === "donutChart";

  const slices = useMemo(() => {
    const f = leg ? projectionFormat(model, leg) : null;
    const lv = data.columns.get("legend:0");
    const keys = leg ? keysOf(data, "legend:0") : [];
    const vv = data.columns.get("values:0");
    const hv = data.columns.get("hl:values:0");
    const out: { name: string; key: string | null; value: number; hl: number | null }[] = [];
    for (let r = 0; r < Math.min(data.rows, MAX_SLICES); r++) {
      const v = numberAt(vv, r);
      if (v === null || v <= 0) continue;
      const raw = lv?.get(r);
      out.push({ name: raw === null || raw === undefined ? t("(Blank)") : f ? fmt(raw, f, locale) : String(raw), key: keys[r] ?? null, value: v, hl: hv ? numberAt(hv, r) : null });
    }
    return { out, highlighted: !!hv };
  }, [data, leg, model, locale, t]);

  const ref = useRef({ slices, interact, leg });
  ref.current = { slices, interact, leg };

  useEffect(() => {
    if (!el.current) return;
    const c = echarts.init(el.current, undefined, { renderer: "canvas", devicePixelRatio: (window.devicePixelRatio || 1) * Math.max(1, scale) });
    chart.current = c;
    // Tests find points through the chart (`convertToPixel`).
    (el.current as unknown as { chart: echarts.ECharts }).chart = c;
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(el.current);
    let onItem = false;
    c.on("click", (p) => {
      onItem = true;
      const params = p as unknown as { data?: { slice?: number }; event?: { event?: MouseEvent } };
      const { slices, interact, leg } = ref.current;
      const s = slices.out[params.data?.slice ?? -1];
      if (s && leg) interact.choose([leg], [s.key], additive(params.event?.event));
    });
    c.getZr().on("click", () => {
      if (!onItem) ref.current.interact.clear();
      onItem = false;
    });
    return () => {
      ro.disconnect();
      c.dispose();
      chart.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const c = chart.current;
    if (!c || !el.current) return;
    const o = visual.objects;
    const colors = palette(el.current);
    const fg = token("--fg-muted", el.current);
    const bg = token("--bg-raised", el.current) || token("--bg", el.current);
    const font = token("--font-ui") || "Inter Variable, sans-serif";
    const vf = val ? projectionFormat(model, val) : { type: "Double" as const, format: null };
    const showLegend = prop<boolean>(o, def, "legend", "show");
    const legendPos = prop<string>(o, def, "legend", "position");
    const labels = prop<string>(o, def, "dataLabels", "content");
    const total = slices.out.reduce((s, x) => s + x.value, 0);
    const color = (i: number) => colors[i % colors.length] ?? "#888";
    // Highlighted: each slice is its highlighted share, then the rest faded.
    const data = slices.out.flatMap((s, i) => {
      const dim = s.key !== undefined && interact.chosen([s.key]) === false;
      const solid = { name: s.name, value: slices.highlighted ? (s.hl ?? 0) : s.value, slice: i, itemStyle: { color: color(i), opacity: dim ? 0.35 : 1 } };
      if (!slices.highlighted) return [solid];
      return [solid, { name: s.name, value: s.value - (s.hl ?? 0), slice: i, rest: true, itemStyle: { color: fade(color(i), 0.3) }, label: { show: false }, labelLine: { show: false } }];
    });
    const pct = (v: number) => new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 1 }).format(total ? v / total : 0);
    c.setOption(
      {
        animation: document.documentElement.dataset.motion !== "off",
        animationDuration: 250,
        textStyle: { fontFamily: font },
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
          data: slices.out.map((s, i) => ({ name: s.name, itemStyle: { color: color(i) } })),
        },
        tooltip: {
          trigger: "item",
          backgroundColor: token("--bg-overlay"),
          borderColor: token("--border"),
          textStyle: { color: token("--fg"), fontSize: 12, fontFamily: font },
          formatter: (p: unknown) => {
            const d = (p as { data: { slice: number } }).data;
            const s = slices.out[d.slice];
            if (!s) return "";
            const hl = slices.highlighted ? `<div style="opacity:.7">${t("highlighted")}: ${s.hl === null ? t("(Blank)") : escape(fmt(s.hl, vf, locale))}</div>` : "";
            return `<div style="font-weight:600">${escape(s.name)}</div><div>${escape(fmt(s.value, vf, locale))} (${pct(s.value)})</div>${hl}`;
          },
        },
        series: [
          {
            type: "pie",
            radius: donut ? ["48%", "72%"] : [0, "72%"],
            center: ["50%", showLegend && legendPos !== "right" ? "54%" : "50%"],
            avoidLabelOverlap: true,
            itemStyle: { borderColor: bg, borderWidth: 1 },
            label: {
              show: labels !== "none",
              color: fg,
              fontSize: 11,
              formatter: (p: { data: { slice: number } }) => {
                const s = slices.out[p.data.slice];
                if (!s) return "";
                if (labels === "percent") return pct(s.value);
                if (labels === "value") return fmt(s.value, vf, locale);
                return `${s.name}\n${pct(s.value)}`;
              },
            },
            labelLine: { lineStyle: { color: token("--border-strong", el.current) } },
            emphasis: { scale: true, scaleSize: 4 },
            data,
          },
        ],
      },
      { notMerge: true },
    );
  }, [slices, visual.objects, def, val, model, locale, t, env, themeKey, donut, interact]);

  return <div ref={el} className="cartesian" />;
}
