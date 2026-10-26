import { describe, expect, it } from "vitest";
import { gaugeRange, kpiState, trend, waterfallSteps } from "./calc";
import { cellLook, gradientColor, mix, ruleFor, stats } from "./conditional";

const opts = { total: true, maxBreakdowns: 2, totalLabel: "Total", otherLabel: "Other" };
const row = (label: string, value: number, breakdown?: string) => ({ label, keys: [label], breakdown: breakdown === undefined ? null : { key: breakdown, label: breakdown }, value });

describe("waterfall", () => {
  it("steps from the running total and ends in the total", () => {
    const s = waterfallSteps([row("a", 10), row("b", -4), row("c", 6)], opts);
    expect(s.map((x) => [x.start, x.end, x.kind])).toEqual([
      [0, 10, "increase"],
      [10, 6, "decrease"],
      [6, 12, "increase"],
      [0, 12, "total"],
    ]);
  });
  it("breaks each change down by member, the rest as Other", () => {
    const s = waterfallSteps([row("2024", 10, "x"), row("2024", 5, "y"), row("2024", 1, "z"), row("2025", 12, "x"), row("2025", 2, "y"), row("2025", 3, "z")], opts);
    expect(s.map((x) => `${x.label}:${x.start}->${x.end}`)).toEqual(["2024:0->16", "y:16->13", "x:13->15", "Other:15->17", "2025:0->17"]);
  });
});

describe("analytics", () => {
  it("fits a trend line through the points", () => {
    expect(trend([1, 2, 3, 4])).toEqual([1, 4]);
    expect(trend([2, null, 2])).toEqual([2, 2]);
    expect(trend([5])).toBeNull();
  });
});

describe("gauge and KPI", () => {
  it("runs the arc to twice the value without a maximum", () => {
    expect(gaugeRange(50, { min: null, max: null, target: null }, { min: "", max: "", target: "" })).toEqual({ min: 0, max: 100, target: null });
    expect(gaugeRange(50, { min: null, max: 80, target: 60 }, { min: "10", max: "", target: "" })).toEqual({ min: 10, max: 80, target: 60 });
  });
  it("compares the last value with its target", () => {
    expect(kpiState([1, 2, 3], [2, 2, 2], true)).toEqual({ value: 3, goal: 2, good: true, distance: 0.5 });
    expect(kpiState([1, 2, null], [2, 2, 2], false).good).toBe(true);
  });
});

describe("conditional formatting", () => {
  const base = { field: null, min: { kind: "lowest" as const, value: 0 }, max: { kind: "highest" as const, value: 0 }, mid: null, minColor: "#000000", midColor: "#808080", maxColor: "#ffffff", rules: [] };
  it("colours a gradient between the lowest and highest values", () => {
    const s = stats([0, 5, 10])!;
    expect(gradientColor({ ...base, mode: "gradient" }, 5, s)).toBe(mix("#000000", "#ffffff", 0.5));
    expect(gradientColor({ ...base, mode: "gradient" }, 10, s)).toBe("rgb(255, 255, 255)");
  });
  it("picks the first rule a value falls in, by number or percent of the range", () => {
    const r = { ...base, mode: "rules" as const, rules: [{ from: 50, to: null, unit: "percent" as const, color: "#00ff00", icon: "up" as const }, { from: null, to: 50, unit: "percent" as const, color: "#ff0000", icon: "down" as const }] };
    const s = { min: 0, max: 200 };
    expect(ruleFor(r, 150, s)?.icon).toBe("up");
    expect(ruleFor(r, 20, s)?.icon).toBe("down");
    const look = cellLook({ icons: r, dataBars: { ...base, mode: "gradient" } }, () => 100, () => s);
    expect(look.icon?.name).toBe("up");
    expect(look.bar).toEqual({ from: 0, to: 50, color: "#4e79a7" });
  });
});
