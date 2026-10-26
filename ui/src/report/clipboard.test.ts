import { describe, expect, it } from "vitest";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";
import { copyPayload, parsePayload, pasteVisuals } from "./clipboard";

const model = (tableId: string, columnId: string, measure = true): Model =>
  ({
    tables: [
      {
        id: tableId,
        name: "Sales",
        columns: [{ id: columnId, name: "Region" }],
        measures: measure ? [{ id: `${tableId}-m`, name: "Total" }] : [],
      },
    ],
    relationships: [],
  }) as unknown as Model;

const page = (visuals: Visual[] = []): Page => ({ id: "p", name: "P", width: 1280, height: 720, filters: [], visuals });

const chart = (m: Model): Visual => ({
  id: "v1",
  visualType: "columnChart",
  position: { x: 100, y: 50, width: 300, height: 200, z: 3 },
  roles: {
    category: [{ field: { kind: "column", table: m.tables[0]!.id, column: m.tables[0]!.columns[0]!.id }, aggregation: null, displayName: null }],
    values: [{ field: { kind: "measure", table: m.tables[0]!.id, measure: `${m.tables[0]!.id}-m` }, aggregation: null, displayName: null }],
  },
  filters: [],
  sort: { role: "values", index: 0, descending: true },
  objects: { title: { text: "Mine" } },
});

describe("pasting visuals", () => {
  it("gives fresh ids and offsets a paste onto the visuals it came from", () => {
    const m = model("t", "c");
    const v = chart(m);
    const clip = parsePayload(JSON.stringify(copyPayload(m, "/a", page([v]), [v])))!;
    const { visuals, dropped } = pasteVisuals(m, page([v]), clip);
    expect(dropped).toBe(0);
    expect(visuals[0]!.id).not.toBe("v1");
    expect(visuals[0]!.position).toMatchObject({ x: 116, y: 66, z: 4 });
    expect(visuals[0]!.roles).toEqual(v.roles);
    // On an empty page it lands where it was.
    expect(pasteVisuals(m, page(), clip).visuals[0]!.position).toMatchObject({ x: 100, y: 50 });
  });

  it("finds fields by name in another model and drops the ones it lacks", () => {
    const from = model("t1", "c1");
    const v = chart(from);
    const clip = copyPayload(from, "/a", page([v]), [v]);
    const to = model("t2", "c2", false);
    const { visuals, dropped } = pasteVisuals(to, page(), clip);
    expect(dropped).toBe(1);
    expect(visuals[0]!.roles).toEqual({ category: [{ field: { kind: "column", table: "t2", column: "c2" }, aggregation: null, displayName: null }] });
    // The sort was on a field that is gone.
    expect(visuals[0]!.sort).toBeNull();
    expect(parsePayload("just some text")).toBeNull();
  });
});
