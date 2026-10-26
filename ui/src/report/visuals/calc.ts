// The arithmetic behind the v1.2 visuals, apart from their renderers (so it can be
// tested without a browser): trend lines, waterfall steps, a gauge's range and a KPI's
// state.

/** Least-squares line through the non-blank points: [value at the first index, at the last]. */
export function trend(values: (number | null)[]): [number, number] | null {
  const pts = values.flatMap((v, i) => (v === null || v === undefined ? [] : [[i, v] as const]));
  if (pts.length < 2) return null;
  const n = pts.length;
  const sx = pts.reduce((s, [x]) => s + x, 0);
  const sy = pts.reduce((s, [, y]) => s + y, 0);
  const sxx = pts.reduce((s, [x]) => s + x * x, 0);
  const sxy = pts.reduce((s, [x, y]) => s + x * y, 0);
  const d = n * sxx - sx * sx;
  if (d === 0) return null;
  const b = (n * sxy - sx * sy) / d;
  const a = (sy - b * sx) / n;
  return [a, a + b * (values.length - 1)];
}

export type StepKind = "increase" | "decrease" | "total";

export interface Step {
  label: string;
  start: number;
  end: number;
  kind: StepKind;
  /** The category's values at the levels shown. */
  keys: (string | null)[];
  /** The breakdown member's key (a breakdown step). */
  breakdown?: string | null;
}

/** The steps of a waterfall from rows of (category, breakdown, value). */
export function waterfallSteps(
  rows: { label: string; keys: (string | null)[]; breakdown: { key: string | null; label: string } | null; value: number | null }[],
  opts: { total: boolean; maxBreakdowns: number; totalLabel: string; otherLabel: string },
): Step[] {
  const steps: Step[] = [];
  const kind = (d: number): StepKind => (d >= 0 ? "increase" : "decrease");
  if (!rows.some((r) => r.breakdown)) {
    let run = 0;
    for (const r of rows) {
      const v = r.value ?? 0;
      steps.push({ label: r.label, start: run, end: run + v, kind: kind(v), keys: r.keys });
      run += v;
    }
    if (opts.total && rows.length) steps.push({ label: opts.totalLabel, start: 0, end: run, kind: "total", keys: [] });
    return steps;
  }
  // Categories in order, each with its breakdown members' values.
  const cats: { label: string; keys: (string | null)[]; by: Map<string, { key: string | null; label: string; value: number }> }[] = [];
  const index = new Map<string, number>();
  for (const r of rows) {
    const id = JSON.stringify(r.keys);
    let i = index.get(id);
    if (i === undefined) {
      i = cats.length;
      index.set(id, i);
      cats.push({ label: r.label, keys: r.keys, by: new Map() });
    }
    const b = r.breakdown ?? { key: null, label: "" };
    cats[i]!.by.set(String(b.key), { key: b.key, label: b.label, value: r.value ?? 0 });
  }
  const sum = (c: (typeof cats)[number]) => [...c.by.values()].reduce((s, x) => s + x.value, 0);
  let run = 0;
  cats.forEach((c, ci) => {
    if (ci > 0) {
      const prev = cats[ci - 1]!;
      const members = new Map([...prev.by].map(([k, v]) => [k, { key: v.key, label: v.label }]));
      for (const [k, v] of c.by) members.set(k, { key: v.key, label: v.label });
      const deltas = [...members].map(([k, m]) => ({ ...m, d: (c.by.get(k)?.value ?? 0) - (prev.by.get(k)?.value ?? 0) })).filter((x) => x.d !== 0);
      deltas.sort((a, b) => Math.abs(b.d) - Math.abs(a.d));
      const top = deltas.slice(0, opts.maxBreakdowns);
      const other = deltas.slice(opts.maxBreakdowns).reduce((s, x) => s + x.d, 0);
      for (const x of top) {
        steps.push({ label: x.label, start: run, end: run + x.d, kind: kind(x.d), keys: c.keys, breakdown: x.key });
        run += x.d;
      }
      if (other) {
        steps.push({ label: opts.otherLabel, start: run, end: run + other, kind: kind(other), keys: c.keys });
        run += other;
      }
    }
    run = sum(c);
    steps.push({ label: c.label, start: 0, end: run, kind: "total", keys: c.keys });
  });
  return steps;
}

/** The arc's range and target: fields first, then the format pane, then defaults. */
export function gaugeRange(value: number | null, fields: { min: number | null; max: number | null; target: number | null }, typed: { min: string; max: string; target: string }) {
  const num = (s: string) => (s.trim() === "" || !Number.isFinite(Number(s)) ? null : Number(s));
  const min = fields.min ?? num(typed.min) ?? 0;
  const target = fields.target ?? num(typed.target);
  const max = fields.max ?? num(typed.max) ?? Math.max(min + 1, (value ?? 0) > 0 ? (value ?? 0) * 2 : 1, target !== null ? target * 1.2 : 0);
  return { min, max, target };
}

/** The KPI's state: its last value and target, and whether it is on target. */
export function kpiState(values: (number | null)[], goals: (number | null)[], highIsGood: boolean) {
  let last = values.length - 1;
  while (last >= 0 && (values[last] === null || values[last] === undefined)) last--;
  const value = last >= 0 ? values[last]! : null;
  const goal = last >= 0 ? (goals[last] ?? null) : (goals.at(-1) ?? null);
  const good = value === null || goal === null ? null : highIsGood ? value >= goal : value <= goal;
  const distance = value === null || goal === null || goal === 0 ? null : (value - goal) / Math.abs(goal);
  return { value, goal, good, distance };
}
