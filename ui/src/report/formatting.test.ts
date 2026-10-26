import { describe, expect, it } from "vitest";
import type { Visual } from "../bridge/generated/Visual";
import { commonObjects, MIXED, paintFormat, shownValue, withProp } from "./formatting";

const visual = (visualType: string, objects: Visual["objects"] = {}): Visual => ({
  id: visualType,
  visualType,
  position: { x: 0, y: 0, width: 100, height: 100, z: 0 },
  roles: {},
  filters: [],
  objects,
});

describe("formatting several visuals", () => {
  it("shows the sections and props every type has", () => {
    const names = commonObjects(["columnChart", "lineChart"]).map((o) => o.name);
    expect(names).toEqual(["title", "legend", "categoryAxis", "valueAxis", "dataLabels", "colors", "background"]);
    expect(commonObjects(["card", "columnChart"]).map((o) => o.name)).toEqual(["title", "background"]);
    // A text box and a shape share nothing.
    expect(commonObjects(["textbox", "shape"])).toEqual([]);
  });

  it("shows a shared value, or mixed, counting each type's defaults", () => {
    const card = visual("card");
    const chart = visual("columnChart");
    // A card's title is off by default, a chart's on.
    expect(shownValue([card, chart], "title", "show")).toBe(MIXED);
    expect(shownValue([withProp(card, "title", "show", true), chart], "title", "show")).toBe(true);
    expect(shownValue([card, chart], "background", "border")).toBe(true);
  });

  it("paints the look but keeps content", () => {
    const source = visual("columnChart", { title: { text: "Sales by month", fontSize: 20 }, legend: { show: false }, colors: { fill: "#ff0000" } });
    const target = visual("barChart", { title: { text: "Mine" }, dataLabels: { show: true } });
    const painted = paintFormat(source, target);
    expect(painted.objects).toEqual({ title: { text: "Mine", fontSize: 20 }, legend: { show: false }, colors: { fill: "#ff0000" } });
    // Only what the target has: a card takes the title and background.
    expect(paintFormat(source, visual("card")).objects).toEqual({ title: { fontSize: 20, show: true } });
  });
});
