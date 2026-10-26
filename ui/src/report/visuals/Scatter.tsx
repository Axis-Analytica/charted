// Scatter (and bubble) charts (ECharts): one point per value of the details field, at
// its X and Y values, sized by the size value, coloured by the legend. Clicking a point
// chooses it; a highlight from elsewhere fades the points it doesn't reach.
import { ScatterChart } from "echarts/charts";
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
import { escape } from "./Cartesian";
import { axisLabel, fade, fmt, numberAt, palette, projectionFormat, token } from "./common";
import { additive, type Interact } from "./interact";

echarts.use([ScatterChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer]);

interface Point {
  label: string;
  legend: string;
  tuple: (string | null)[];
  x: number;
  y: number;
  size: number | null;
  lit: boolean;
}

export function Scatter({ visual, model, data, interact }: { visual: Visual; model: Model; data: VisualData; interact: Interact }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const env = useApp((s) => s.env);
  const themeKey = useContext(ThemeKey);
  const scale = useContext(CanvasScale);
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const def = visualDef("scatterChart");
  const details = visual.roles.details?.[0];
  const legend = visual.roles.legend?.[0];
  const xp = visual.roles.x?.[0];
  const yp = visual.roles.y?.[0];
  const sp = visual.roles.size?.[0];
  const fields: Projection[] = [details, legend].filter((p): p is Projection => !!p);

  const shaped = useMemo(() => {
    const text = (role: string, p: Projection | undefined, r: number) => {
      if (!p) return "";
      const v = data.columns.get(`${role}:0`)?.get(r);
      return v === null || v === undefined ? t("(Blank)") : fmt(v, projectionFormat(model, p), locale);
    };
    const dk = details ? keysOf(data, "details:0") : [];
    const lk = legend ? keysOf(data, "legend:0") : [];
    const groups = new Map<string, Point[]>();
    let maxSize = 0;
    for (let r = 0; r < data.rows; r++) {
      const x = numberAt(data.columns.get("x:0"), r);
      const y = numberAt(data.columns.get("y:0"), r);
      if (x === null || y === null) continue;
      const size = sp ? numberAt(data.columns.get("size:0"), r) : null;
      if (size !== null) maxSize = Math.max(maxSize, Math.abs(size));
      const lit = !data.highlighted || numberAt(data.columns.get("hl:y:0"), r) !== null || numberAt(data.columns.get("hl:x:0"), r) !== null;
      const tuple = [...(details ? [dk[r] ?? null] : []), ...(legend ? [lk[r] ?? null] : [])];
      const lg = legend ? text("legend", legend, r) : "";
      const pt = { label: details ? text("details", details, r) : lg, legend: lg, tuple, x, y, size, lit };
      const list = groups.get(lg) ?? [];
      list.push(pt);
      groups.set(lg, list);
    }
    return { groups: [...groups.entries()], maxSize };
  }, [data, details, legend, sp, model, locale, t]);

  const ref = useRef({ shaped, interact, fields });
  ref.current = { shaped, interact, fields };

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
      const params = p as unknown as { seriesIndex: number; dataIndex: number; event?: { event?: MouseEvent } };
      const { shaped, interact, fields } = ref.current;
      const pt = shaped.groups[params.seriesIndex]?.[1][params.dataIndex];
      if (pt && fields.length) interact.choose(fields, pt.tuple, additive(params.event?.event));
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
    const faint = token("--fg-faint", el.current);
    const border = token("--border", el.current);
    const font = token("--font-ui") || "Inter Variable, sans-serif";
    const xf = xp ? projectionFormat(model, xp) : { type: "Double" as const, format: null };
    const yf = yp ? projectionFormat(model, yp) : { type: "Double" as const, format: null };
    const sf = sp ? projectionFormat(model, sp) : null;
    const showLegend = prop<boolean>(o, def, "legend", "show") && shaped.groups.length > 1;
    const legendPos = prop<string>(o, def, "legend", "position");
    const markerSize = prop<number>(o, def, "markers", "size");
    const axis = (p: Projection | undefined, f: typeof xf) => ({
      type: "value" as const,
      scale: true,
      name: p && prop<boolean>(o, def, "axes", "titles") ? projectionName(model, p, t) : undefined,
      nameLocation: "middle" as const,
      nameGap: 28,
      nameTextStyle: { color: faint, fontFamily: font },
      splitLine: { show: prop<boolean>(o, def, "axes", "gridlines"), lineStyle: { color: border, opacity: 0.6 } },
      axisLine: { lineStyle: { color: border } },
      axisLabel: { color: fg, fontFamily: font, fontSize: 11, formatter: (v: number) => axisLabel(v, f, locale) },
    });
    const size = (v: number | null) => (v === null || !shaped.maxSize ? markerSize : 6 + 34 * Math.sqrt(Math.abs(v) / shaped.maxSize));
    c.setOption(
      {
        animation: document.documentElement.dataset.motion !== "off",
        animationDuration: 250,
        textStyle: { fontFamily: font },
        grid: { left: 8, right: 16, top: showLegend && legendPos === "top" ? 32 : 16, bottom: showLegend && legendPos === "bottom" ? 32 : 8, containLabel: true },
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
        },
        tooltip: {
          trigger: "item",
          backgroundColor: token("--bg-overlay"),
          borderColor: border,
          textStyle: { color: token("--fg"), fontSize: 12, fontFamily: font },
          formatter: (p: unknown) => {
            const q = p as { seriesIndex: number; dataIndex: number };
            const pt = shaped.groups[q.seriesIndex]?.[1][q.dataIndex];
            if (!pt) return "";
            const rows = [
              xp ? `${escape(projectionName(model, xp, t))}: <b>${escape(fmt(pt.x, xf, locale))}</b>` : "",
              yp ? `${escape(projectionName(model, yp, t))}: <b>${escape(fmt(pt.y, yf, locale))}</b>` : "",
              sp && sf && pt.size !== null ? `${escape(projectionName(model, sp, t))}: <b>${escape(fmt(pt.size, sf, locale))}</b>` : "",
            ].filter(Boolean);
            return `<div style="font-weight:600">${escape(pt.label)}</div>${rows.map((r) => `<div>${r}</div>`).join("")}`;
          },
        },
        xAxis: axis(xp, xf),
        yAxis: axis(yp, yf),
        series: shaped.groups.map(([name, pts], gi) => {
          const color = colors[gi % colors.length] ?? "#888";
          return {
            type: "scatter" as const,
            name: name || (yp ? projectionName(model, yp, t) : ""),
            color,
            data: pts.map((p) => {
              const faded = !p.lit || interact.chosen(p.tuple) === false;
              return { value: [p.x, p.y], symbolSize: size(p.size), itemStyle: { color: faded ? fade(color, 0.25) : fade(color, 0.85), borderColor: token("--bg-raised", el.current), borderWidth: 1 } };
            }),
            emphasis: { focus: "self" as const },
          };
        }),
      },
      { notMerge: true },
    );
  }, [shaped, visual.objects, def, xp, yp, sp, model, locale, t, env, themeKey, interact.chosen]);

  return <div ref={el} className="cartesian" />;
}
