// The setup every ECharts renderer shares: an instance on a container (at enough pixels
// for the canvas zoom, resized with it, exposed as `el.chart` for tests), its theme
// tokens, and the click handling that chooses, drills or clears.
import * as echarts from "echarts/core";
import { useContext, useEffect, useRef } from "react";
import type { Projection } from "../../bridge/generated/Projection";
import { CanvasScale } from "../scale";
import { token } from "./common";
import { additive, type Interact } from "./interact";

/** A data point's fields and values (what choosing it filters on), or null. */
export type PointOf = (params: { dataIndex: number; seriesIndex: number; data?: unknown }) => { fields: Projection[]; tuple: (string | null)[]; drill?: (string | null)[] } | null;

export function useEChart(interact: Interact, pointOf: PointOf) {
  const scale = useContext(CanvasScale);
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const ref = useRef({ interact, pointOf });
  ref.current = { interact, pointOf };
  useEffect(() => {
    if (!el.current) return;
    const c = echarts.init(el.current, undefined, { renderer: "canvas", devicePixelRatio: (window.devicePixelRatio || 1) * Math.max(1, scale) });
    chart.current = c;
    (el.current as unknown as { chart: echarts.ECharts }).chart = c;
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(el.current);
    let onItem = false;
    type Params = { componentType?: string; dataIndex: number; seriesIndex: number; data?: unknown; event?: { event?: MouseEvent } };
    c.on("click", (p) => {
      const params = p as unknown as Params;
      if (params.componentType !== "series") return;
      onItem = true;
      const { interact, pointOf } = ref.current;
      const pt = pointOf(params);
      if (!pt) return;
      if (interact.drill && pt.drill?.length) interact.drill(pt.drill);
      else if (pt.fields.length) interact.choose(pt.fields, pt.tuple, additive(params.event?.event));
    });
    c.on("contextmenu", (p) => {
      const params = p as unknown as Params;
      if (params.componentType !== "series") return;
      onItem = true;
      const { interact, pointOf } = ref.current;
      const pt = pointOf(params);
      const ev = params.event?.event;
      if (!pt || !ev || !pt.fields.length || !interact.menu) return;
      ev.preventDefault();
      interact.menu(pt.fields, pt.tuple, { x: ev.clientX, y: ev.clientY });
    });
    c.getZr().on("click", () => {
      if (!onItem) ref.current.interact.clear();
      onItem = false;
    });
    c.getZr().on("contextmenu", () => {
      onItem = false;
    });
    return () => {
      ro.disconnect();
      c.dispose();
      chart.current = null;
    };
    // The container remounts this component when the zoom changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return { el, chart };
}

/** Theme colours and fonts for a chart under `el`. */
export function chartTheme(el: Element) {
  // The theme's label font, set on the page, else the UI's.
  const font = token("--report-font", el) || token("--font-ui") || "Inter Variable, sans-serif";
  return {
    font,
    fg: token("--fg-muted", el),
    strong: token("--fg-strong", el) || token("--fg", el),
    faint: token("--fg-faint", el),
    border: token("--border", el),
    bg: token("--bg-raised", el) || token("--bg", el),
    success: token("--success", el) || "#3a9a5b",
    danger: token("--danger", el) || "#d0453a",
    tooltip: { backgroundColor: token("--bg-overlay"), borderColor: token("--border", el), textStyle: { color: token("--fg"), fontSize: 12, fontFamily: font } },
    animation: document.documentElement.dataset.motion !== "off",
  };
}
