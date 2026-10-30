// The agent's catalog of visual types (crates/agent/src/catalog.json) is generated from
// the registry, so the tools it builds visuals with accept what the panes do. After
// changing the registry or the built-in themes, regenerate it:
//   CHARTED_UPDATE_CATALOG=1 bin/heavy pnpm -C ui exec vitest run catalog
import { readFileSync, writeFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { VISUALS } from "./registry";
import { BUILT_IN } from "./themes";

const FILE = new URL("../../../crates/agent/src/catalog.json", import.meta.url);

function catalog() {
  return {
    visuals: VISUALS.map((v) => ({
      type: v.type,
      label: v.label,
      roles: v.roles.map((r) => ({ name: r.name, label: r.label, kind: r.kind, max: r.max ?? null })),
      size: v.size,
      decor: !!v.decor,
      objects: v.objects
        .filter((o) => o.props.length)
        .map((o) => ({
          name: o.name,
          props: o.props.map((p) => ({
            name: p.name,
            type: p.type,
            default: p.default,
            ...("min" in p && p.min !== undefined ? { min: p.min } : {}),
            ...("max" in p && p.max !== undefined ? { max: p.max } : {}),
            ...("options" in p ? { options: p.options.map((o) => o.value) } : {}),
          })),
        })),
    })),
    themes: BUILT_IN.map((t) => t.json),
  };
}

describe("the agent's catalog", () => {
  it("matches the registry", () => {
    const text = JSON.stringify(catalog(), null, 1) + "\n";
    if (process.env.CHARTED_UPDATE_CATALOG) writeFileSync(FILE, text);
    expect(readFileSync(FILE, "utf8")).toBe(text);
  });
});
