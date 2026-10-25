import { useEffect, useRef, useState } from "react";

export interface FrameStats {
  p50: number;
  p95: number;
  max: number;
  long: number;
}

/** Measures requestAnimationFrame intervals over a sliding window. */
export function useFrameStats(windowSize = 240): FrameStats {
  const [stats, setStats] = useState<FrameStats>({ p50: 0, p95: 0, max: 0, long: 0 });
  const samples = useRef<number[]>([]);
  useEffect(() => {
    let last = performance.now();
    let raf = 0;
    let tick = 0;
    const loop = (now: number) => {
      const dt = now - last;
      last = now;
      const s = samples.current;
      s.push(dt);
      if (s.length > windowSize) s.shift();
      if (++tick % 30 === 0 && s.length > 10) {
        const sorted = [...s].sort((a, b) => a - b);
        const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] ?? 0;
        setStats({ p50: at(0.5), p95: at(0.95), max: sorted[sorted.length - 1] ?? 0, long: s.filter((x) => x > 33).length });
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [windowSize]);
  return stats;
}

export function FrameMeter() {
  const s = useFrameStats();
  return (
    <span className="meter" data-testid="frame-meter">
      frame p50 {s.p50.toFixed(1)} ms · p95 {s.p95.toFixed(1)} ms · max {s.max.toFixed(0)} ms · &gt;33ms {s.long}/240
    </span>
  );
}
