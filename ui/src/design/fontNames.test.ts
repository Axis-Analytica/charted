import { describe, expect, it } from "vitest";
import { fontStack } from "./fontNames";

describe("font stacks", () => {
  it("adds the bundled face and a stand-in for Power BI's fonts", () => {
    expect(fontStack("Lora")).toBe('Lora, "Lora Variable", var(--font-ui, sans-serif)');
    expect(fontStack("Segoe UI")).toBe('"Segoe UI", "Open Sans Variable", var(--font-ui, sans-serif)');
    expect(fontStack("'Segoe UI', wf_segoe-ui_normal, helvetica, sans-serif")).toBe('"Segoe UI", "Open Sans Variable", wf_segoe-ui_normal, helvetica, var(--font-ui, sans-serif)');
  });
  it("keeps a user's font as it is, and strips what would break the CSS", () => {
    expect(fontStack("Brand Sans")).toBe('"Brand Sans", var(--font-ui, sans-serif)');
    expect(fontStack('Evil"; } body { color: red')).toBe('"Evil  body  color: red", var(--font-ui, sans-serif)');
    expect(fontStack("")).toBeUndefined();
  });
});
