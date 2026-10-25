// Drilling a visual down its hierarchy (the fields of its axis or legend). The drill
// path is saved with the visual (the core shows the levels it selects, filtered to the
// members drilled into); drill mode, where a click drills instead of selecting, is
// view state.
import { create } from "zustand";
import type { DrillStep } from "../bridge/generated/DrillStep";
import type { Projection } from "../bridge/generated/Projection";
import type { Visual } from "../bridge/generated/Visual";
import { useApp } from "./app";
import { editVisual } from "./report";

/** The role whose fields form a visual type's hierarchy (as `drill_role` in Rust). */
export function drillRole(visualType: string): string | null {
  if (["columnChart", "barChart", "stackedColumnChart", "stackedBarChart", "lineChart"].includes(visualType)) return "category";
  if (visualType === "pieChart" || visualType === "donutChart") return "legend";
  return null;
}

export interface Hierarchy {
  role: string;
  fields: Projection[];
  /** The levels shown: `start..=depth`. */
  start: number;
  depth: number;
  /** Indexes of the levels shown. */
  shown: number[];
  canUp: boolean;
  canDown: boolean;
}

/** The visual's hierarchy and the levels it shows (as `Visual::drill_levels` in Rust);
 * a role with one field shows it. */
export function hierarchy(visual: Visual): Hierarchy | null {
  const role = drillRole(visual.visualType);
  const fields = role ? (visual.roles[role] ?? []) : [];
  if (!role || !fields.length) return null;
  const steps = (visual.drill ?? []).slice(0, Math.max(0, fields.length - 1));
  let start = 0;
  steps.forEach((s, i) => {
    if (s.kind !== "expand") start = i + 1;
  });
  const depth = steps.length;
  const shown = Array.from({ length: depth - start + 1 }, (_, i) => start + i);
  return { role, fields, start, depth, shown, canUp: depth > 0, canDown: depth < fields.length - 1 };
}

function setDrill(page: string, visual: string, label: string, fn: (steps: DrillStep[]) => DrillStep[]) {
  return useApp.getState().edit(label, editVisual(page, visual, (v) => ({ ...v, drill: fn(v.drill ?? []) })));
}

export const drillUp = (page: string, visual: Visual) => setDrill(page, visual.id, "Drill up", (s) => s.slice(0, -1));
export const drillNext = (page: string, visual: Visual) => setDrill(page, visual.id, "Go to the next level", (s) => [...s, { kind: "next" }]);
export const expandDown = (page: string, visual: Visual) => setDrill(page, visual.id, "Expand all down one level", (s) => [...s, { kind: "expand" }]);
/** Drills into a member: its values at the levels shown. */
export const drillInto = (page: string, visual: Visual, keys: (string | null)[]) => setDrill(page, visual.id, "Drill down", (s) => [...s, { kind: "into", keys }]);

/** Visuals in drill mode: a click on a data point drills into it. */
export const useDrillMode = create<{ on: string[]; toggle(id: string): void; off(id: string): void }>((set) => ({
  on: [],
  toggle: (id) => set((s) => ({ on: s.on.includes(id) ? s.on.filter((x) => x !== id) : [...s.on, id] })),
  off: (id) => set((s) => ({ on: s.on.filter((x) => x !== id) })),
}));
