import { describe, expect, it } from "vitest";
import { parseBlocks, parseInline } from "./Markdown";

describe("markdown", () => {
  it("reads emphasis, nested and escaped", () => {
    expect(parseInline("a **bold *and italic*** b")).toEqual([
      { kind: "text", text: "a " },
      { kind: "bold", children: [{ kind: "text", text: "bold " }, { kind: "italic", children: [{ kind: "text", text: "and italic" }] }] },
      { kind: "text", text: " b" },
    ]);
    expect(parseInline("\\*not\\* ~~gone~~ `x*y`")).toEqual([
      { kind: "text", text: "*not* " },
      { kind: "strike", children: [{ kind: "text", text: "gone" }] },
      { kind: "text", text: " " },
      { kind: "code", text: "x*y" },
    ]);
  });
  it("leaves unclosed markers, spaced stars and snake_case alone", () => {
    expect(parseInline("2 * 3 and **open")).toEqual([{ kind: "text", text: "2 * 3 and **open" }]);
    expect(parseInline("total_sales_ytd")).toEqual([{ kind: "text", text: "total_sales_ytd" }]);
  });
  it("reads links", () => {
    expect(parseInline("see [the docs](https://x.org/a)")).toEqual([
      { kind: "text", text: "see " },
      { kind: "link", url: "https://x.org/a", children: [{ kind: "text", text: "the docs" }] },
    ]);
  });
  it("reads headings, lists and paragraphs with line breaks", () => {
    const b = parseBlocks("# Sales\nline one\nline two\n\n- a\n- **b**\n1. one");
    expect(b.map((x) => x.kind)).toEqual(["heading", "paragraph", "list", "list"]);
    expect(b[1]).toMatchObject({ kind: "paragraph", lines: [[{ text: "line one" }], [{ text: "line two" }]] });
    expect(b[2]).toMatchObject({ kind: "list", ordered: false });
    expect(b[3]).toMatchObject({ kind: "list", ordered: true, items: [[{ text: "one" }]] });
  });
});
