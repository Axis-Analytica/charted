import { describe, expect, it } from "vitest";
import type { Model } from "../bridge/generated/Model";
import type { Table } from "../bridge/generated/Table";
import { folderPath, folderTree, hierarchyFields, moveMeasure, tableFolders, uniqueFieldName, withLevel } from "./fieldTree";

const table = {
  id: "t1",
  name: "Sales",
  source: "sales",
  columns: [
    { id: "c1", name: "Year", dataType: "Int64", kind: { kind: "data", sourceColumn: "Year" }, summarizeBy: "Default", hidden: false, isKey: false, displayFolder: "Time" },
    { id: "c2", name: "Month", dataType: "String", kind: { kind: "data", sourceColumn: "Month" }, summarizeBy: "Default", hidden: false, isKey: false, displayFolder: "Time\\Calendar" },
  ],
  measures: [{ id: "m1", name: "Total", expression: "1", language: "Dax", hidden: false, displayFolder: " Money \\ " }],
  hierarchies: [{ id: "h1", name: "Dates", levels: [{ name: "Year", column: "c1" }, { name: "Gone", column: "cx" }], hidden: false, displayFolder: "time" }],
  hidden: false,
  isDateTable: false,
} as unknown as Table;

describe("display folders", () => {
  it("nest with backslashes", () => {
    expect(folderPath("A\\ B \\\\C")).toEqual(["A", "B", "C"]);
    expect(folderPath(null)).toEqual([]);
    const tree = folderTree([...table.measures, ...table.columns, ...table.hierarchies!], (x) => x.displayFolder);
    expect(tree.items).toEqual([]);
    expect(tree.folders.map((f) => f.path)).toEqual(["Money", "Time"]);
    const time = tree.folders[1]!;
    // Folder names match without regard to case.
    expect(time.items.map((x) => x.name)).toEqual(["Year", "Dates"]);
    expect(time.folders[0]!.path).toBe("Time\\Calendar");
    expect(time.folders[0]!.items.map((x) => x.name)).toEqual(["Month"]);
    expect(tableFolders(table)).toEqual(["Money", "Time", "time", "Time\\Calendar"].sort((a, b) => a.localeCompare(b)));
  });
});

describe("hierarchies", () => {
  it("add their levels' columns", () => {
    expect(hierarchyFields(table, table.hierarchies![0]!)).toEqual([{ kind: "column", table: "t1", column: "c1" }]);
    const h = withLevel(table.hierarchies![0]!, { ...table.columns[1]!, name: "Gone" });
    expect(h.levels.map((l) => l.name)).toEqual(["Year", "Gone", "Gone 2"]);
    expect(withLevel(h, table.columns[0]!)).toBe(h);
    expect(uniqueFieldName(table, "dates")).toBe("dates 2");
    expect(uniqueFieldName(table, "Other")).toBe("Other");
  });
});

describe("moving a measure", () => {
  it("rewrites the report's references", () => {
    const ref = { kind: "measure", table: "t1", measure: "m1" };
    const visual = { id: "v1", visualType: "card", position: {}, roles: { values: [{ field: ref, aggregation: null }] }, filters: [], objects: {} };
    const other = { id: "v2", visualType: "card", position: {}, roles: {}, filters: [], objects: {} };
    const model = {
      tables: [table, { ...table, id: "t2", name: "Other", measures: [] }],
      report: { pages: [{ id: "p1", name: "P", width: 1, height: 1, filters: [{ field: ref }], visuals: [visual, other] }], filters: [], diagram: {}, bookmarks: [] },
    } as unknown as Model;
    const changes = moveMeasure(model, "m1", "t1", "t2");
    expect(changes.map((c) => c.op)).toEqual(["removeMeasure", "addMeasure", "updateVisual", "setPage"]);
    const moved = changes[2] as unknown as { visual: typeof visual };
    expect(moved.visual.roles.values[0]!.field).toEqual({ ...ref, table: "t2" });
    expect(visual.roles.values[0]!.field.table).toBe("t1");
    expect(moveMeasure(model, "m1", "t1", "t1")).toEqual([]);
  });
});
