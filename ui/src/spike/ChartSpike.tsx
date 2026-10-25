import { LineChart, ScatterChart } from "echarts/charts";
import { GridComponent, TooltipComponent, DataZoomComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useEffect, useRef, useState } from "react";

echarts.use([ScatterChart, LineChart, GridComponent, TooltipComponent, DataZoomComponent, CanvasRenderer]);

const N = 100_000;

export function ChartSpike() {
  const scatterRef = useRef<HTMLDivElement>(null);
  const lineRef = useRef<HTMLDivElement>(null);
  const [renderMs, setRenderMs] = useState<number | null>(null);
  useEffect(() => {
    if (!scatterRef.current || !lineRef.current) return;
    const pts = new Float64Array(N * 2);
    for (let i = 0; i < N; i++) {
      const x = Math.random() * 100;
      pts[i * 2] = x;
      pts[i * 2 + 1] = x * 0.6 + (Math.random() - 0.5) * 40;
    }
    const line: [number, number][] = [];
    let v = 100;
    for (let i = 0; i < N; i++) {
      v += (Math.random() - 0.49) * 2;
      line.push([i, v]);
    }
    const t0 = performance.now();
    const a = echarts.init(scatterRef.current);
    a.setOption({
      animation: false,
      grid: { left: 40, right: 16, top: 16, bottom: 30 },
      xAxis: { type: "value" },
      yAxis: { type: "value" },
      series: [{ type: "scatter", large: true, largeThreshold: 2000, symbolSize: 2, data: pts, dimensions: ["x", "y"] }],
    });
    const b = echarts.init(lineRef.current);
    b.setOption({
      animation: false,
      grid: { left: 40, right: 16, top: 16, bottom: 50 },
      tooltip: { trigger: "axis" },
      xAxis: { type: "value" },
      yAxis: { type: "value", scale: true },
      dataZoom: [{ type: "inside" }, { type: "slider" }],
      series: [{ type: "line", showSymbol: false, sampling: "lttb", data: line }],
    });
    requestAnimationFrame(() => setRenderMs(performance.now() - t0));
    const ro = new ResizeObserver(() => {
      a.resize();
      b.resize();
    });
    ro.observe(scatterRef.current);
    return () => {
      ro.disconnect();
      a.dispose();
      b.dispose();
    };
  }, []);
  return (
    <div className="split">
      <p className="note">100,000-point scatter + 100,000-point line (LTTB sampling, zoomable). First render: {renderMs?.toFixed(0) ?? "…"} ms</p>
      <div ref={scatterRef} className="chart" />
      <div ref={lineRef} className="chart" />
    </div>
  );
}
