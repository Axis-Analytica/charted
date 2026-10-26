// The quick measure templates write the DAX crates/core/tests/quickmeasures.rs checks on
// the sample (keep the two in step).
import { describe, expect, it } from "vitest";
import { type FieldOption, type Picks, quickMeasure } from "./quickMeasures";

const field = (ref: string, kind: "measure" | "column", table: string, numeric = true, date = false): FieldOption => ({ ref, label: ref, short: ref.replace(/^.*\[|\]$/g, ""), kind, numeric, date, table });
const fields = [field("[Total]", "measure", "Sales"), field("Sales[UnitPrice]", "column", "Sales"), field("Sales[Quantity]", "column", "Sales"), field("'Date'[Date]", "column", "Date", false, true), field("Product[Category]", "column", "Product", false)];
const picks: Picks = { value: "[Total]", second: "[Total]", column: "Product[Category]", table: "Sales", date: "'Date'[Date]", category: "Product[Category]", text: "Bikes", weight: "Sales[Quantity]" };

describe("quick measures", () => {
  it("write the DAX the core checks", () => {
    const dax = (c: Parameters<typeof quickMeasure>[0], p: Partial<Picks> = {}) => quickMeasure(c, { ...picks, ...p }, fields).expression;
    expect(dax("yoyChange")).toBe("[Total] - CALCULATE ( [Total], SAMEPERIODLASTYEAR ( 'Date'[Date] ) )");
    expect(dax("yoyPercent")).toBe("VAR prior = CALCULATE ( [Total], SAMEPERIODLASTYEAR ( 'Date'[Date] ) ) RETURN DIVIDE ( [Total] - prior, prior )");
    expect(dax("ytd")).toBe("TOTALYTD ( [Total], 'Date'[Date] )");
    expect(dax("shareOfTotal")).toBe("DIVIDE ( [Total], CALCULATE ( [Total], ALLSELECTED ( Product[Category] ) ) )");
    expect(dax("averagePer")).toBe("AVERAGEX ( VALUES ( Product[Category] ), [Total] )");
    expect(dax("maxPer")).toBe("MAXX ( VALUES ( Product[Category] ), [Total] )");
    expect(dax("weightedAverage", { value: "Sales[UnitPrice]" })).toBe("DIVIDE ( SUMX ( Sales, Sales[UnitPrice] * Sales[Quantity] ), SUM ( Sales[Quantity] ) )");
    expect(dax("filtered")).toBe('CALCULATE ( [Total], Product[Category] = "Bikes" )');
    // A column is summed.
    expect(dax("sum", { value: "Sales[UnitPrice]" })).toBe("SUM ( Sales[UnitPrice] )");
  });

  it("name them, and format percentages", () => {
    const m = quickMeasure("yoyPercent", picks, fields);
    expect(m.name).toBe("Total YoY %");
    expect(m.format).toBe("0.0%");
    expect(quickMeasure("filtered", { ...picks, text: 'Say "hi"' }, fields).expression).toContain('"Say ""hi"""');
  });
});
