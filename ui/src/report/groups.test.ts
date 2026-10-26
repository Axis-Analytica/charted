// The group dialog writes the DAX crates/core/tests/groups.rs evaluates on the sample
// (keep the two in step).
import { describe, expect, it } from "vitest";
import { type BinGroup, groupDax, groupKind, groupOf, sizeForCount, withGroup } from "./groups";

const bin = (unit: BinGroup["unit"], size: number): BinGroup => ({ kind: "bin", column: "c", unit, size, by: "size", count: 10 });

describe("groups and bins", () => {
  it("write bins as INT arithmetic", () => {
    expect(groupDax(bin("number", 500), "Sales", "Amount", "Double")).toBe("IF ( ISBLANK ( 'Sales'[Amount] ), BLANK (), INT ( 'Sales'[Amount] / 500 ) * 500 )");
    expect(groupDax(bin("number", 0.05), "Sales", "Discount", "Double")).toBe("IF ( ISBLANK ( 'Sales'[Discount] ), BLANK (), ROUND ( INT ( 'Sales'[Discount] / 0.05 ) * 0.05, 2 ) )");
    expect(groupDax(bin("year", 2), "Date", "Date", "Date")).toBe("IF ( ISBLANK ( 'Date'[Date] ), BLANK (), DATE ( INT ( YEAR ( 'Date'[Date] ) / 2 ) * 2, 1, 1 ) )");
    expect(groupDax(bin("month", 3), "Date", "Date", "Date")).toBe(
      "IF ( ISBLANK ( 'Date'[Date] ), BLANK (), DATE ( 1900, INT ( ( ( YEAR ( 'Date'[Date] ) - 1900 ) * 12 + MONTH ( 'Date'[Date] ) - 1 ) / 3 ) * 3 + 1, 1 ) )",
    );
    expect(groupDax(bin("day", 7), "Date", "Date", "Date")).toBe("IF ( ISBLANK ( 'Date'[Date] ), BLANK (), DATE ( 1900, 1, 1 + INT ( DATEDIFF ( DATE ( 1900, 1, 1 ), 'Date'[Date], DAY ) / 7 ) * 7 ) )");
  });

  it("write lists as SWITCH with IN", () => {
    const spec = { kind: "list" as const, column: "c", groups: [{ name: "Outdoor", values: ["Camping", "Packs"] }, { name: 'Say "hi"', values: ["Footwear"] }, { name: "Empty", values: [] }], other: "Other" };
    expect(groupDax(spec, "Product", "Category", "String")).toBe(
      'SWITCH (\n    TRUE (),\n    \'Product\'[Category] IN { "Camping", "Packs" }, "Outdoor",\n    \'Product\'[Category] IN { "Footwear" }, "Say ""hi""",\n    "Other"\n)',
    );
    expect(groupDax({ ...spec, other: null }, "Product", "Category", "String")).toContain("\"Say \"\"hi\"\"\",\n    'Product'[Category]\n)");
    expect(groupDax({ ...spec, groups: [] }, "Product", "Category", "String")).toBe('"Other"');
    expect(groupDax({ ...spec, groups: [], other: null }, "O'Neil", "A]b", "String")).toBe("'O''Neil'[A]]b]");
  });

  it("pick the kind and sizes", () => {
    expect(groupKind("Double")).toBe("bin");
    expect(groupKind("Date")).toBe("bin");
    expect(groupKind("String")).toBe("list");
    expect(sizeForCount("number", 10, 0, 1234, true)).toBe(124);
    expect(sizeForCount("number", 10, 0, 1.234, false)).toBe(0.13);
    expect(sizeForCount("number", 4, 5, 5, false)).toBe(1);
    expect(sizeForCount("day", 10, 0, 365, false)).toBe(37);
  });

  it("keep the settings in the column's extras", () => {
    const spec = bin("number", 5);
    const extras = withGroup({ other: 1, charted: { keep: true } }, spec);
    expect(extras).toEqual({ other: 1, charted: { keep: true, group: spec } });
    expect(groupOf({ extras } as never)).toEqual(spec);
    expect(groupOf({ extras: {} } as never)).toBeNull();
  });
});
