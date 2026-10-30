// The user's layout of charted (crates/core/src/workspace.rs): where each pane goes, a
// second view beside the first, and the commands hidden from toolbars and menus. It lives
// in the core (so the agent can change it too) and is read here from the app state.
import type { Layout } from "../bridge/generated/Layout";
import type { PaneLayout } from "../bridge/generated/PaneLayout";
import type { Place } from "../bridge/generated/Place";
import type { Workspace } from "../bridge/generated/Workspace";
import { msg } from "../i18n";
import { useApp } from "../state/app";

export const PANE_IDS = ["bookmarks", "selection", "sync", "filters", "visualizations", "fields", "agent"] as const;
export type PaneId = (typeof PANE_IDS)[number];

/** The built-in profiles' names (the core names them; translated when shown). */
export const BUILTIN_NAMES = [msg("Standard"), msg("Report + model"), msg("Two report pages")];

/** Counts uses of the commands people click (anything marked `data-command`). */
export function installUseCounter(): () => void {
  const click = (e: MouseEvent) => {
    const el = (e.target as HTMLElement | null)?.closest<HTMLElement>("[data-command]");
    if (el?.dataset.command) countUse(el.dataset.command);
  };
  document.addEventListener("click", click, true);
  return () => document.removeEventListener("click", click, true);
}

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

export const useLayout = () => useApp((s) => activeLayout(s.state?.workspace));
export const useWorkspace = () => useApp((s) => s.state?.workspace ?? null);

/** Changes the active profile's layout (one layout-undo step). */
export function setLayout(change: (l: Layout) => Layout): Promise<boolean> {
  const l = activeLayout(useApp.getState().state?.workspace);
  return useApp.getState().rpc({ type: "setLayout", layout: change(l) });
}

// How often each hideable command is used on this machine (for "hide what I never use").
const USES = "charted.commandUses";

export function commandUses(): Record<string, number> {
  try {
    return JSON.parse(localStorage.getItem(USES) ?? "{}") as Record<string, number>;
  } catch {
    return {};
  }
}

/** Counts a use of a command (a click on an element with `data-command`). */
export function countUse(id: string) {
  try {
    const u = commandUses();
    u[id] = (u[id] ?? 0) + 1;
    localStorage.setItem(USES, JSON.stringify(u));
  } catch {
    // Storage can be unavailable; counting is only a hint.
  }
}
