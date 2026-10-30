import { describe, expect, it } from "vitest";
import type { AppState } from "../bridge/generated/AppState";
import { newer } from "./revision";

const at = (revision: number) => ({ revision }) as AppState;

describe("states from the core", () => {
  it("keep the newer of a reply and a pushed state", () => {
    expect(newer(null, at(0))).toBe(true);
    expect(newer(at(3), at(4))).toBe(true);
    // A read (state request) doesn't count as a change.
    expect(newer(at(3), at(3))).toBe(true);
    expect(newer(at(5), at(4))).toBe(false);
  });
});
