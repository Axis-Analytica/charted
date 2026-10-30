// The layout's pure logic (app/layout.ts reads and changes it in the app state).
import type { Layout } from "../bridge/generated/Layout";
import type { PaneLayout } from "../bridge/generated/PaneLayout";
import type { Place } from "../bridge/generated/Place";
import type { Workspace } from "../bridge/generated/Workspace";

export const PANE_IDS = ["bookmarks", "selection", "sync", "filters", "visualizations", "fields", "agent"] as const;
export type PaneId = (typeof PANE_IDS)[number];

const EMPTY: Layout = { panes: {}, hidden: [] };

/** A pane's place when the layout doesn't say (as the core's `default_place`). */
export const defaultPlace = (pane: string): Place => (pane === "filters" || pane === "visualizations" || pane === "fields" ? "right" : "hidden");

export function activeLayout(w: Workspace | null | undefined): Layout {
  if (!w) return EMPTY;
  return (w.profiles.find((p) => p.id === w.active) ?? w.profiles[0])?.layout ?? EMPTY;
}

export function paneOf(l: Layout, pane: string): PaneLayout {
  return l.panes[pane] ?? { place: defaultPlace(pane) };
}

/** `l` with `pane` changed by `patch`. */
export function withPane(l: Layout, pane: string, patch: Partial<PaneLayout>): Layout {
  return { ...l, panes: { ...l.panes, [pane]: { ...paneOf(l, pane), ...patch } } };
}

/** Shows a hidden pane (docked right) or hides a shown one. */
export function togglePane(l: Layout, pane: string): Layout {
  return withPane(l, pane, { place: paneOf(l, pane).place === "hidden" ? "right" : "hidden" });
}

export function withHidden(l: Layout, ids: string[], hide: boolean): Layout {
  const set = new Set(l.hidden);
  for (const id of ids) {
    if (hide) set.add(id);
    else set.delete(id);
  }
  return { ...l, hidden: [...set].sort() };
}

