import { describe, expect, it } from "vitest";
import type { Workspace } from "../bridge/generated/Workspace";
import { activeLayout, paneOf, togglePane, withHidden, withPane } from "./layout";

const ws: Workspace = {
  active: "b",
  profiles: [
    { id: "a", name: "A", layout: { panes: {}, hidden: [] } },
    { id: "b", name: "B", layout: { panes: { agent: { place: "float" } }, hidden: ["x"] } },
  ],
};

describe("layouts", () => {
  it("read the active profile, with default places", () => {
    const l = activeLayout(ws);
    expect(paneOf(l, "agent").place).toBe("float");
    expect(paneOf(l, "fields").place).toBe("right");
    expect(paneOf(l, "bookmarks").place).toBe("hidden");
    expect(activeLayout(null).hidden).toEqual([]);
  });
  it("toggle, move and hide", () => {
    let l = activeLayout(ws);
    l = togglePane(l, "fields");
    expect(paneOf(l, "fields").place).toBe("hidden");
    l = togglePane(l, "fields");
    expect(paneOf(l, "fields").place).toBe("right");
    l = withPane(l, "fields", { place: "left", width: 300 });
    expect(paneOf(l, "fields")).toEqual({ place: "left", width: 300 });
    expect(withHidden(l, ["y", "x"], true).hidden).toEqual(["x", "y"]);
    expect(withHidden(l, ["x"], false).hidden).toEqual([]);
  });
});
