// The calculation group dialog's checks and the table it commits.
import { describe, expect, it } from "vitest";
import { calculationGroupTable, itemsProblem, SELECTED_MEASURE } from "./calcgroups";

const item = (name: string, expression = SELECTED_MEASURE, formatString: string | null = null) => ({ name, expression, formatString });

describe("calculation groups", () => {
  it("check their items", () => {
    expect(itemsProblem([item("Current"), item("YTD", "CALCULATE ( SELECTEDMEASURE (), DATESYTD ( 'Date'[Date] ) )")])).toBeNull();
    expect(itemsProblem([])).toContain("at least one");
    expect(itemsProblem([item(" ")])).toContain("name");
    expect(itemsProblem([item("A"), item("a ")])).toContain("same name");
    expect(itemsProblem([item("A", " ")])).toContain("expression");
  });

  it("make a table of the items, trimmed", () => {
    const table = calculationGroupTable("Time", { precedence: 2, items: [item(" PY ", " SELECTEDMEASURE () ", " "), item("Share", SELECTED_MEASURE, '"0%"')] }, () => "t1");
    expect(table).toMatchObject({ id: "t1", name: "Time", columns: [], calculationGroup: { precedence: 2 } });
    expect(table.calculated).toBeUndefined();
    expect(table.calculationGroup!.items).toEqual([item("PY"), item("Share", SELECTED_MEASURE, '"0%"')]);
  });
});
