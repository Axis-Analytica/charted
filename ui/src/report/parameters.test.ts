// The what-if parameter dialog's checks and the model changes it commits.
import { describe, expect, it } from "vitest";
import { type ParameterSpec, parameterProblem, parameterTable } from "./parameters";

const spec = (p: Partial<ParameterSpec> = {}): ParameterSpec => ({ name: "Discount", decimal: false, min: 0, max: 50, step: 5, default: 10, ...p });
let n = 0;
const newId = () => `id${++n}`;

describe("what-if parameters", () => {
  it("check the range", () => {
    expect(parameterProblem(spec())).toBeNull();
    expect(parameterProblem(spec({ step: 0 }))).toContain("increment");
    expect(parameterProblem(spec({ max: -1 }))).toContain("maximum");
    expect(parameterProblem(spec({ default: 60 }))).toContain("default");
    expect(parameterProblem(spec({ step: 0.5 }))).toContain("whole");
    expect(parameterProblem(spec({ decimal: true, step: 0.5 }))).toBeNull();
    expect(parameterProblem(spec({ max: 1e6, step: 1 }))).toContain("10,000");
  });

  it("make a series table and a SELECTEDVALUE measure", () => {
    const table = parameterTable(spec(), newId);
    expect(table.calculated?.expression).toBe("GENERATESERIES ( 0, 50, 5 )");
    expect(table.columns).toHaveLength(1);
    expect(table.columns[0]).toMatchObject({ name: "Discount", dataType: "Int64", kind: { kind: "data", sourceColumn: "Value" } });
    expect(table.measures[0]).toMatchObject({ name: "Discount Value", expression: "SELECTEDVALUE ( Discount[Discount], 10 )" });
    expect(new Set([table.id, table.columns[0]!.id, table.measures[0]!.id]).size).toBe(3);
  });

  it("quote names and format decimals", () => {
    const table = parameterTable(spec({ name: "Growth % [est]", decimal: true, min: 0, max: 0.3, step: 0.05, default: 0.1 }), newId);
    expect(table.measures[0]!.expression).toBe("SELECTEDVALUE ( 'Growth % [est]'[Growth % [est]]], 0.1 )");
    expect(table.columns[0]).toMatchObject({ dataType: "Decimal", formatString: "0.00" });
  });
});
