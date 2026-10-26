// What buttons and page navigators do: go back, show a bookmark, go to a page, or drill
// through with the data point selected on the page. They act on a click in reading
// view and on Ctrl+click while editing (a plain click selects them), as in Power BI.
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";
import { msg } from "../i18n";
import { drillBack, drillTargets, drillThrough, useDrillThrough } from "../state/drillthrough";
import { useReport } from "../state/report";
import { useSelection } from "../state/selection";
import { toast } from "../state/toasts";
import { showBookmark } from "./BookmarksPane";
import { prop, visualDef } from "./registry";

export type ActionType = "none" | "back" | "bookmark" | "pageNavigation" | "drillthrough";

export interface ButtonAction {
  type: ActionType;
  bookmark: string;
  page: string;
  tooltip: string;
}

/** Visual types that act when clicked. */
export const isActionVisual = (type: string) => type === "actionButton" || type === "pageNavigator";

/** A button's action: its own type, or the one its icon stands for. */
export function buttonAction(v: Visual): ButtonAction {
  const def = visualDef("actionButton");
  const on = prop<boolean>(v.objects, def, "action", "show");
  const own = prop<string>(v.objects, def, "action", "type");
  const kind = prop<string>(v.objects, def, "button", "kind");
  const fromIcon: Record<string, ActionType> = { back: "back", bookmark: "bookmark", pageNavigation: "pageNavigation", drillthrough: "drillthrough" };
  const type: ActionType = !on ? "none" : own === "auto" ? (fromIcon[kind] ?? "none") : (own as ActionType);
  return { type, bookmark: prop<string>(v.objects, def, "action", "bookmark"), page: prop<string>(v.objects, def, "action", "page"), tooltip: prop<string>(v.objects, def, "action", "tooltip") };
}

/** Pages a navigator lists: every page, or every page not hidden. */
export function navigatorPages(model: Model, showHidden: boolean): Page[] {
  return model.report.pages.filter((p) => showHidden || !p.hidden);
}

// The pages visited, for Back.
const history: string[] = [];
let goingBack = false;
useReport.subscribe((s, prev) => {
  if (s.page === prev.page || !prev.page) return;
  if (goingBack) goingBack = false;
  else history.push(prev.page);
  if (history.length > 50) history.shift();
});

/** Goes to a page as a button does (leaving any drill through). */
export function goToPage(id: string) {
  useReport.getState().set({ page: id, selected: [] });
}

/** Back: out of a drill through, else to the page shown before. */
export function goBack(model: Model) {
  if (useDrillThrough.getState().current) return drillBack();
  while (history.length) {
    const id = history.pop()!;
    if (id !== useReport.getState().page && model.report.pages.some((p) => p.id === id)) {
      goingBack = true;
      useReport.getState().set({ page: id, selected: [] });
      return;
    }
  }
}

const NEED_POINT = msg("Select a data point first");
const NO_TARGET = msg("Choose the button's drill-through page in its Action settings");

/** Runs a button's action on `page`. */
export function runAction(model: Model, page: Page, a: ButtonAction, t: (s: string) => string) {
  switch (a.type) {
    case "back":
      return goBack(model);
    case "bookmark": {
      const b = (model.report.bookmarks ?? []).find((x) => x.id === a.bookmark);
      if (b) void showBookmark(model, b);
      return;
    }
    case "pageNavigation":
      if (model.report.pages.some((p) => p.id === a.page)) goToPage(a.page);
      return;
    case "drillthrough": {
      const target = model.report.pages.find((p) => p.id === a.page);
      if (!target) return toast({ kind: "info", title: t(NO_TARGET) });
      const sel = useSelection.getState().selection;
      const source = sel && sel.page === page.id ? page.visuals.find((v) => v.id === sel.source) : undefined;
      if (!sel || !source || !sel.tuples[0] || !drillTargets(model, page, sel.fields).some((p) => p.id === target.id)) return toast({ kind: "info", title: t(NEED_POINT) });
      return drillThrough(page, source, sel.fields, sel.tuples[0], target);
    }
    case "none":
      return;
  }
}
