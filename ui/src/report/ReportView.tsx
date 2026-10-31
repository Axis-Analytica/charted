import {
  ArrowLeft,
  Sparkles,
  BookOpen,
  Bookmark as BookmarkIcon,
  Check,
  MousePointerClick,
  Paintbrush,
  Palette,
  Pencil,
  Sheet,
  AlignCenterHorizontal,
  AlignCenterVertical,
  AlignEndHorizontal,
  AlignEndVertical,
  AlignHorizontalDistributeCenter,
  AlignStartHorizontal,
  AlignStartVertical,
  AlignVerticalDistributeCenter,
  BringToFront,
  Filter,
  Layers,
  ListTree,
  MessageSquare,
  Plus,
  RefreshCw,
  SendToBack,
  Shapes,
} from "lucide-react";
import { Fragment, type ReactNode, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";
import { type Shortcut, useShortcut } from "../app/shortcuts";
import { msg, useT } from "../i18n";
import { IconButton } from "../kit/Button";
import { useApp } from "../state/app";
import { currentPage, newId, newVisual, pageSettings, updateVisual, useReport } from "../state/report";
import { Canvas, usePageTheme } from "./Canvas";
import { modelSignature, useVisualData } from "./data";
import { CanvasScale, ThemeKey } from "./scale";
import { BUILT_IN } from "./themes";
import { copyPayload, pasteVisuals, readClipboard, writeClipboard } from "./clipboard";
import { paint, usePainter } from "./painter";
import { toast } from "../state/toasts";
import { DataTable, VisualContainer } from "./VisualContainer";
import { Menu } from "../kit/Menu";
import { Button } from "../kit/Button";
import { useSelection } from "../state/selection";
import { drillBack, useDrillThrough } from "../state/drillthrough";
import { BookmarksPane, useBookmarkShortcuts } from "./BookmarksPane";
import { projectionName } from "../state/report";
import { transport } from "../bridge";
import { FieldsPane } from "./FieldsPane";
import { FiltersPane } from "./FiltersPane";
import { FormulaBar } from "./FormulaBar";
import { QuickMeasureDialog } from "./QuickMeasure";
import { PrintLayout } from "./PrintLayout";
import { ManageRolesDialog, RolesMenu, useRolesDialog, ViewAsBanner, ViewAsDialog } from "./Roles";
import { ThemeEditor, useThemeEditor } from "./ThemeEditor";
import { SelectionPane } from "./SelectionPane";
import { SyncSlicersPane } from "./SyncSlicersPane";
import { VISUALS } from "./registry";
import { VisualizationsPane } from "./VisualizationsPane";
import "./report.css";
import { AgentPane } from "../agent/AgentPane";
import { PANE_IDS, type PaneId, paneOf, setLayout, togglePane as togglePaneIn, useLayout, withHidden } from "../app/layout";
import { ContextMenu, type ContextMenuState } from "../kit/ContextMenu";
import { PaneHost } from "./PaneHost";
import { ReportSlot } from "./slot";
import { TOOLBAR_COMMANDS } from "./toolbar";

type Align = "left" | "center" | "right" | "top" | "middle" | "bottom";

/** Aligns or distributes visuals within their combined bounds. */
function arrange(visuals: Visual[], how: Align | "distributeH" | "distributeV"): Visual[] {
  const left = Math.min(...visuals.map((v) => v.position.x));
  const top = Math.min(...visuals.map((v) => v.position.y));
  const right = Math.max(...visuals.map((v) => v.position.x + v.position.width));
  const bottom = Math.max(...visuals.map((v) => v.position.y + v.position.height));
  const at = (v: Visual, x: number, y: number) => ({ ...v, position: { ...v.position, x: Math.round(x), y: Math.round(y) } });
  if (how === "distributeH" || how === "distributeV") {
    const h = how === "distributeH";
    const sorted = [...visuals].sort((a, b) => (h ? a.position.x - b.position.x : a.position.y - b.position.y));
    const total = sorted.reduce((s, v) => s + (h ? v.position.width : v.position.height), 0);
    const gap = ((h ? right - left : bottom - top) - total) / Math.max(1, sorted.length - 1);
    let cursor = h ? left : top;
    return sorted.map((v) => {
      const out = h ? at(v, cursor, v.position.y) : at(v, v.position.x, cursor);
      cursor += (h ? v.position.width : v.position.height) + gap;
      return out;
    });
  }
  return visuals.map((v) => {
    const p = v.position;
    switch (how) {
      case "left": return at(v, left, p.y);
      case "center": return at(v, (left + right) / 2 - p.width / 2, p.y);
      case "right": return at(v, right - p.width, p.y);
      case "top": return at(v, p.x, top);
      case "middle": return at(v, p.x, (top + bottom) / 2 - p.height / 2);
      case "bottom": return at(v, p.x, bottom - p.height);
    }
  });
}

function PageTabs({ model, page, reading }: { model: Model; page: Page; reading: boolean }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const slot = useContext(ReportSlot);
  const setReport = useReport((s) => s.set);
  const set = ({ page: id, selected }: { page: string; selected: string[] }) => setReport(slot ? { page2: id, selected, slot } : { page: id, selected, slot });
  const [renaming, setRenaming] = useState<string | null>(null);
  const settings = pageSettings;
  const addPage = async () => {
    const names = new Set(model.report.pages.map((p) => p.name.toLowerCase()));
    let n = model.report.pages.length + 1;
    while (names.has(t("Page %1", n).toLowerCase())) n++;
    const id = newId();
    // A new page takes the current page's size, unless that is a (small) tooltip page.
    const p: Page = { id, name: t("Page %1", n), width: page.tooltip ? 1280 : page.width, height: page.tooltip ? 720 : page.height, filters: [], interactions: [], visuals: [] };
    if (await commit("Add page", [{ op: "addPage", page: p, index: null }])) set({ page: id, selected: [] });
  };
  return (
    <nav className="page-tabs" aria-label={t("Pages")}>
      <div role="tablist" className="page-tab-list">
        {model.report.pages.map((p, i) => (reading && p.hidden && p.id !== page.id ? null : (
          <div key={p.id} className={`page-tab ${p.id === page.id ? "is-on" : ""} ${p.hidden ? "is-hidden" : ""} ${p.tooltip ? "is-tooltip" : ""}`} title={p.hidden && !reading ? t("Hidden page") : undefined}>
            {renaming === p.id ? (
              <input
                className="page-tab-input"
                autoFocus
                defaultValue={p.name}
                aria-label={t("Page name")}
                onFocus={(e) => e.target.select()}
                onBlur={(e) => {
                  setRenaming(null);
                  const name = e.target.value.trim();
                  if (name && name !== p.name) commit("Rename page", [{ op: "setPage", page: p.id, settings: { ...settings(p), name } }]);
                }}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                  if (e.key === "Escape") setRenaming(null);
                }}
              />
            ) : (
              <button
                type="button"
                role="tab"
                aria-selected={p.id === page.id}
                onClick={() => set({ page: p.id, selected: [] })}
                onDoubleClick={() => !reading && setRenaming(p.id)}
                onKeyDown={(e) => {
                  if (reading) return;
                  if (e.key === "F2") setRenaming(p.id);
                  if (e.key === "Delete" && model.report.pages.length > 1) commit("Delete page", [{ op: "removePage", page: p.id }]);
                  if (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
                    e.preventDefault();
                    const to = i + (e.key === "ArrowLeft" ? -1 : 1);
                    if (to >= 0 && to < model.report.pages.length) commit("Move page", [{ op: "movePage", page: p.id, index: to }]);
                  }
                }}
              >
                {p.name}
                {p.tooltip && <MessageSquare size={11} className="page-tab-tip" aria-hidden />}
              </button>
            )}
          </div>
        )))}
      </div>
      {!reading && <IconButton label={t("New page")} icon={<Plus size={14} />} onPress={addPage} />}
    </nav>
  );
}

/** One visual over the whole canvas area (focus mode), with its data below it when
 * shown as a table. */
function FocusView({ model, page, visual, table, signature }: { model: Model; page: Page; visual: Visual; table: boolean; signature: string }) {
  const { t } = useT();
  const set = useReport((s) => s.set);
  const theme = usePageTheme(model);
  const { data } = useVisualData(visual, page, model, signature, visual.visualType === "table");
  const focus = { visual: visual.id, table };
  return (
    <div className="focus-view" role="dialog" aria-label={focus.table ? t("Show as a table") : t("Focus mode")}>
      <div className="focus-bar">
        <Button variant="ghost" size="sm" icon={<ArrowLeft size={14} />} onPress={() => set({ focus: null })}>
          {t("Back to report")}
        </Button>
        <span className="status-fill" />
        <IconButton label={t("Show as a table")} icon={<Sheet size={15} />} pressed={focus.table} onPress={() => set({ focus: { ...focus, table: !focus.table } })} />
      </div>
      <div className={`focus-body ${focus.table ? "with-table" : ""}`} style={theme.style}>
        <CanvasScale.Provider value={1}>
          <ThemeKey.Provider value={theme.key}>
            <VisualContainer visual={visual} page={page} model={model} signature={signature} selected={false} register={() => {}} onPointerDown={() => {}} standalone />
          </ThemeKey.Provider>
        </CanvasScale.Provider>
        {focus.table && data && <DataTable visual={visual} model={model} data={data} />}
      </div>
    </div>
  );
}

/** The report theme: the desktop's (live), a built-in one, or a Power BI theme file. */
function ThemeMenu({ model }: { model: Model }) {
  const { t } = useT();
  const rpc = useApp((s) => s.rpc);
  const commit = useApp((s) => s.commit);
  const current = model.report.theme?.name ?? null;
  const mark = (on: boolean) => (on ? <Check size={14} /> : null);
  return (
    <Menu
      label={t("Themes")}
      trigger={<Palette size={15} />}
      items={[
        { id: "omarchy", label: t("Omarchy (follows the desktop)"), icon: mark(!model.report.theme), run: () => model.report.theme && commit("Change theme", [{ op: "setTheme", theme: null }]) },
        ...BUILT_IN.map((th) => ({ id: `theme-${th.name}`, label: th.name, icon: mark(current === th.name), run: () => commit("Change theme", [{ op: "setTheme", theme: th }]) })),
        ...(current && !BUILT_IN.some((b) => b.name === current) ? [{ id: "current", label: current, icon: mark(true), run: () => {} }] : []),
        null,
        { id: "customize", label: t("Customize current theme…"), run: () => useThemeEditor.getState().set(true) },
        {
          id: "browse",
          label: t("Browse for themes…"),
          run: async () => {
            const path = await transport.pick({ title: t("Import a Power BI theme"), filters: [{ name: t("Power BI theme"), extensions: ["json"] }] });
            if (path) await rpc({ type: "importTheme", path });
          },
        },
      ]}
    />
  );
}

export function ReportView({ model }: { model: Model }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const slot = useContext(ReportSlot);
  const { page: page1, page2, slot: focused, selected, formula, set, select, editInteractions, focus, reading, view } = useReport();
  const pageId = slot ? page2 : page1;
  // The panes live in the first report view, or the second when the first is another view.
  const hostsPanes = slot === 0 || view !== "report";
  // Whether this view is on screen (a second report view is only rendered when shown).
  const onScreen = () => slot === 1 || useReport.getState().view === "report";
  const page = currentPage(model, pageId);
  const layout = useLayout();
  const split = layout.split?.view === "report";
  // The panes act on the report view last used.
  const panePage = (split && focused === 1 ? currentPage(model, page2) : currentPage(model, page1)) ?? page;
  const signature = useMemo(() => modelSignature(model), [model]);
  // Sets the theme's format defaults before the canvas and the format pane read them.
  usePageTheme(model);
  const pageRef = useRef(page);
  pageRef.current = page;
  // In narrow windows (a tiled half screen) the three panes share one tabbed column.
  const body = useRef<HTMLDivElement>(null);
  const [compact, setCompact] = useState(false);
  const [paneTab, setPaneTab] = useState<(typeof PANES)[number]>("visualizations");
  const [toolMenu, setToolMenu] = useState<ContextMenuState | null>(null);
  // The toolbar's commands in the command palette (built below; read when one runs).
  const toolsRef = useRef<Tool[]>([]);
  useShortcut(
    ...(slot !== focused ? [] : TOOLBAR_COMMANDS).map(({ id, label }) => ({
      id: `toolbar-${id}`,
      keys: "",
      label,
      group: "View" as const,
      run: () => toolsRef.current.find((x) => x.id === id)?.run?.(),
      enabled: () => {
        const x = toolsRef.current.find((y) => y.id === id);
        return !!x?.run && !x.disabled;
      },
    })),
  );
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setCompact(el.clientWidth < 1240));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    if (page && page.id !== pageId) set(slot ? { page2: page.id } : { page: page.id });
  }, [page, pageId, set, slot]);
  // Focus mode ends when its visual goes away (undo, another page).
  const focusVisual = focus ? page?.visuals.find((v) => v.id === focus.visual) : undefined;
  useEffect(() => {
    if (focus && !focusVisual) set({ focus: null });
  }, [focus, focusVisual, set]);
  // A selection belongs to its page.
  useEffect(() => {
    const s = useSelection.getState().selection;
    if (s && s.page !== page?.id) useSelection.getState().clear();
  }, [page?.id]);

  const chosen = page ? page.visuals.filter((v) => selected.includes(v.id)) : [];
  const apply = (label: string, visuals: Visual[]) => page && visuals.length && commit(label, visuals.map((v) => updateVisual(page.id, v)));
  const nudge = (dx: number, dy: number) => {
    const p = pageRef.current;
    if (!p) return;
    const vs = p.visuals.filter((v) => useReport.getState().selected.includes(v.id));
    apply(
      "Move visual",
      vs.map((v) => ({
        ...v,
        position: { ...v.position, x: Math.max(0, Math.min(p.width - v.position.width, v.position.x + dx)), y: Math.max(0, Math.min(p.height - v.position.height, v.position.y + dy)) },
      })),
    );
  };
  // Canvas keys only act when focus is on the canvas, not in a pane or field.
  const canvasFocused = () => {
    const a = document.activeElement;
    return !a || a === document.body || !!a.closest(".report-center");
  };
  const hasSelection = () => useReport.getState().selected.length > 0 && onScreen() && !useReport.getState().reading && canvasFocused();
  const zOrder = (front: boolean) => {
    if (!page) return;
    const zs = page.visuals.map((v) => v.position.z);
    const edge = front ? Math.max(...zs) + 1 : Math.min(...zs) - 1;
    apply(front ? "Bring to front" : "Send to back", chosen.map((v, i) => ({ ...v, position: { ...v.position, z: edge + (front ? i : -i) } })));
  };

  const painter = usePainter();
  const lastPaintPress = useRef(0);
  /** The toolbar's format painter: copies the one selected visual's look for the next
   * visual clicked; a second press within 400ms (a double-click) keeps it on. */
  const pressPainter = () => {
    const now = Date.now();
    const double = now - lastPaintPress.current < 400;
    lastPaintPress.current = now;
    if (painter.armed && double && !painter.sticky) return painter.set({ sticky: true });
    if (painter.armed) return painter.set({ armed: false, sticky: false });
    const source = chosen.length === 1 ? chosen[0]! : null;
    if (source) painter.set({ source: { visualType: source.visualType, objects: source.objects ?? {} }, armed: true, sticky: false });
  };
  const copyFormat = () => {
    const p = pageRef.current;
    const v = p?.visuals.find((x) => x.id === useReport.getState().selected[0]);
    if (v) usePainter.getState().set({ source: { visualType: v.visualType, objects: v.objects ?? {} } });
  };

  /** Copies the selected visuals to the clipboard (and removes them for a cut). */
  const copyVisuals = async (cut: boolean) => {
    const p = pageRef.current;
    const project = useApp.getState().state?.project;
    if (!p || !project) return;
    const vs = p.visuals.filter((v) => useReport.getState().selected.includes(v.id));
    if (!vs.length) return;
    await writeClipboard(copyPayload(project.model, project.path, p, vs));
    if (cut) await commit("Cut visuals", vs.map((v): Change => ({ op: "removeVisual", page: p.id, visual: v.id })));
  };
  const pasteClipboard = async () => {
    const clip = await readClipboard();
    const p = pageRef.current;
    const project = useApp.getState().state?.project;
    if (!clip || !p || !project) return;
    const { visuals, dropped } = pasteVisuals(project.model, p, clip);
    if (!visuals.length) return;
    if (await commit(visuals.length > 1 ? "Paste visuals" : "Paste visual", visuals.map((v): Change => ({ op: "addVisual", page: p.id, visual: v, index: null })))) select(visuals.map((v) => v.id));
    if (dropped) toast({ kind: "info", title: t("Some fields aren't in this model"), detail: t("%1 fields were left out of the pasted visuals.", dropped) });
  };
  const canPaste = () => onScreen() && !useReport.getState().reading && canvasFocused() && !window.getSelection()?.toString();

  useBookmarkShortcuts(model);
  // The report view last used takes the keyboard (a second one beside it shares it).
  useShortcut(...(slot !== focused ? [] : ([
    {
      id: "agent-pane",
      keys: "Ctrl+I",
      label: "Agent pane",
      group: "View",
      run: () => {
        const opening = paneOf(layout, "agent").place === "hidden";
        void setLayout((l) => togglePaneIn(l, "agent"));
        if (opening) {
          setPaneTab("agent");
          requestAnimationFrame(() => requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>(".agent-input")?.focus()));
        }
      },
      enabled: () => onScreen() && !useReport.getState().reading,
    },
    { id: "visual-copy", keys: "Ctrl+C", label: "Copy selected visuals", group: "Edit", run: () => void copyVisuals(false), enabled: () => hasSelection() && !window.getSelection()?.toString() },
    { id: "visual-cut", keys: "Ctrl+X", label: "Cut selected visuals", group: "Edit", run: () => void copyVisuals(true), enabled: () => hasSelection() && !window.getSelection()?.toString() },
    { id: "visual-paste", keys: "Ctrl+V", label: "Paste visuals", group: "Edit", run: () => void pasteClipboard(), enabled: canPaste },
    { id: "painter-off", keys: "Escape", label: "Turn off the format painter", group: "Edit", run: () => usePainter.getState().set({ armed: false, sticky: false }), enabled: () => usePainter.getState().armed },
    { id: "format-copy", keys: "Ctrl+Alt+C", label: "Copy formatting", group: "Edit", run: copyFormat, enabled: () => useReport.getState().selected.length === 1 && hasSelection() },
    {
      id: "format-paste",
      keys: "Ctrl+Alt+V",
      label: "Paste formatting",
      group: "Edit",
      run: () => pageRef.current && void paint(pageRef.current.id, useReport.getState().selected),
      enabled: () => !!usePainter.getState().source && hasSelection(),
    },
    { id: "visual-delete", keys: "Delete", label: "Delete selected visuals", group: "Edit", run: () => page && commit("Remove visuals", useReport.getState().selected.map((id): Change => ({ op: "removeVisual", page: page.id, visual: id }))), enabled: hasSelection },
    { id: "visual-left", keys: "ArrowLeft", label: "Nudge left", group: "Edit", run: () => nudge(-1, 0), enabled: hasSelection },
    { id: "visual-right", keys: "ArrowRight", label: "Nudge right", group: "Edit", run: () => nudge(1, 0), enabled: hasSelection },
    { id: "visual-up", keys: "ArrowUp", label: "Nudge up", group: "Edit", run: () => nudge(0, -1), enabled: hasSelection },
    { id: "visual-down", keys: "ArrowDown", label: "Nudge down", group: "Edit", run: () => nudge(0, 1), enabled: hasSelection },
    { id: "visual-left8", keys: "Shift+ArrowLeft", label: "Move left by 8", group: "Edit", run: () => nudge(-8, 0), enabled: hasSelection },
    { id: "visual-right8", keys: "Shift+ArrowRight", label: "Move right by 8", group: "Edit", run: () => nudge(8, 0), enabled: hasSelection },
    { id: "visual-up8", keys: "Shift+ArrowUp", label: "Move up by 8", group: "Edit", run: () => nudge(0, -8), enabled: hasSelection },
    { id: "visual-down8", keys: "Shift+ArrowDown", label: "Move down by 8", group: "Edit", run: () => nudge(0, 8), enabled: hasSelection },
    { id: "visual-all", keys: "Ctrl+A", label: "Select all visuals", group: "Edit", run: () => page && select(page.visuals.filter((v) => !v.hidden).map((v) => v.id)), enabled: () => onScreen() && canvasFocused() },
    {
      id: "visual-none",
      keys: "Escape",
      label: "Clear the selection",
      group: "Edit",
      run: () => {
        if (useReport.getState().focus) return set({ focus: null });
        select([]);
        useSelection.getState().clear();
      },
      enabled: () => onScreen() && canvasFocused() && (useReport.getState().selected.length > 0 || !!useSelection.getState().selection || !!useReport.getState().focus),
    },
    {
      id: "visual-duplicate",
      keys: "Ctrl+D",
      label: "Duplicate selected visuals",
      group: "Edit",
      run: async () => {
        if (!page) return;
        const copies = page.visuals
          .filter((v) => useReport.getState().selected.includes(v.id))
          .map((v) => ({ ...v, id: newId(), position: { ...v.position, x: Math.min(page.width - v.position.width, v.position.x + 16), y: Math.min(page.height - v.position.height, v.position.y + 16) } }));
        if (await commit("Duplicate visuals", copies.map((v): Change => ({ op: "addVisual", page: page.id, visual: v, index: null })))) select(copies.map((v) => v.id));
      },
      enabled: hasSelection,
    },
    { id: "manage-roles", keys: "", label: "Manage roles…", group: "Edit", run: () => useRolesDialog.getState().set("manage"), enabled: () => onScreen() },
    { id: "view-as", keys: "", label: "View as roles…", group: "View", run: () => useRolesDialog.getState().set("viewAs"), enabled: () => onScreen() && !!model.roles?.length },
    { id: "new-measure", keys: "Ctrl+M", label: "New measure", group: "Edit", run: () => model.tables[0] && set({ formula: `new:${(useReport.getState().field?.table ?? model.tables[0].id)}` }), enabled: () => model.tables.length > 0 },
    { id: "new-quick-measure", keys: "", label: "New quick measure…", group: "Edit", run: () => model.tables[0] && set({ quick: useReport.getState().field?.table ?? model.tables[0].id }), enabled: () => model.tables.length > 0 },
    // Every visual type can be added from the command palette.
    ...VISUALS.map((d) => ({
      id: `add-visual-${d.type}`,
      keys: "",
      label: d.label,
      group: "Edit" as const,
      run: async () => {
        if (!page) return;
        const v = newVisual(page, d.type, useReport.getState().pointer);
        if (await commit("Add visual", [{ op: "addVisual", page: page.id, visual: v, index: null }])) select([v.id]);
      },
      enabled: () => onScreen() && !!page,
    })),
  ] as Shortcut[])));

  if (!page) return <div className="main-empty">{t("This report has no pages.")}</div>;
  const align = (how: Align | "distributeH" | "distributeV") => apply(how.startsWith("distribute") ? "Distribute visuals" : "Align visuals", arrange(chosen, how));
  const togglePane = (k: PaneId) => void setLayout((l) => togglePaneIn(l, k));
  const placeOf = (k: PaneId) => paneOf(layout, k).place;
  // The Agent pane is a conversation: it stays in view rather than behind a tab.
  const docked = (k: PaneId) => k !== "agent" && (placeOf(k) === "left" || placeOf(k) === "right");
  const shown = (k: PaneId) => placeOf(k) !== "hidden";
  const hidden = new Set(layout.hidden);
  const paneLabel = (k: PaneId) =>
    k === "bookmarks" ? t("Bookmarks") : k === "selection" ? t("Selection") : k === "sync" ? t("Sync slicers") : k === "filters" ? t("Filters") : k === "visualizations" ? t("Visualizations") : k === "agent" ? t("Agent") : t("Fields");
  const renderPane = (k: PaneId) => {
    const pg = panePage!;
    if (k === "bookmarks") return <BookmarksPane model={model} page={pg} />;
    if (k === "selection") return <SelectionPane model={model} page={pg} />;
    if (k === "sync") return <SyncSlicersPane model={model} page={pg} />;
    if (k === "filters") return <FiltersPane model={model} page={pg} />;
    if (k === "visualizations") return <VisualizationsPane model={model} page={pg} />;
    if (k === "agent") return <AgentPane model={model} page={pg} />;
    return <FieldsPane model={model} page={pg} />;
  };
  const host = (k: PaneId) => (
    <PaneHost key={k} id={k} label={paneLabel(k)} pane={paneOf(layout, k)}>
      {renderPane(k)}
    </PaneHost>
  );
  // The toolbar's commands: each can be hidden (layout), and all are in the command palette.
  const tools: Tool[] = [
    { id: "align-left", label: "Align left", icon: <AlignStartVertical size={15} />, disabled: chosen.length < 2, run: () => align("left") },
    { id: "align-center", label: "Align center", icon: <AlignCenterVertical size={15} />, disabled: chosen.length < 2, run: () => align("center") },
    { id: "align-right", label: "Align right", icon: <AlignEndVertical size={15} />, disabled: chosen.length < 2, run: () => align("right") },
    { id: "align-top", label: "Align top", icon: <AlignStartHorizontal size={15} />, disabled: chosen.length < 2, run: () => align("top") },
    { id: "align-middle", label: "Align middle", icon: <AlignCenterHorizontal size={15} />, disabled: chosen.length < 2, run: () => align("middle") },
    { id: "align-bottom", label: "Align bottom", icon: <AlignEndHorizontal size={15} />, disabled: chosen.length < 2, run: () => align("bottom") },
    { id: "distribute-h", label: "Distribute horizontally", icon: <AlignHorizontalDistributeCenter size={15} />, disabled: chosen.length < 3, run: () => align("distributeH") },
    { id: "distribute-v", label: "Distribute vertically", icon: <AlignVerticalDistributeCenter size={15} />, disabled: chosen.length < 3, run: () => align("distributeV"), sep: true },
    { id: "bring-front", label: "Bring to front", icon: <BringToFront size={15} />, disabled: !chosen.length, run: () => zOrder(true) },
    { id: "send-back", label: "Send to back", icon: <SendToBack size={15} />, disabled: !chosen.length, run: () => zOrder(false) },
    {
      id: "format-painter",
      label: painter.sticky ? msg("Format painter (stays on; Esc turns it off)") : "Format painter",
      shortcut: "Ctrl+Alt+C",
      icon: <Paintbrush size={15} />,
      pressed: painter.armed,
      disabled: !painter.armed && chosen.length !== 1,
      run: pressPainter,
      sep: true,
    },
    { id: "edit-interactions", label: "Edit interactions", icon: <MousePointerClick size={15} />, pressed: editInteractions, run: () => set({ editInteractions: !editInteractions }) },
    { id: "theme", label: "Theme", node: <ThemeMenu model={model} /> },
    { id: "security", label: "Security", node: <RolesMenu model={model} /> },
    { id: "reading-view", label: "Reading view", icon: <BookOpen size={15} />, run: () => set({ reading: true, selected: [], editInteractions: false }), fill: true },
    { id: "pane-bookmarks", label: "Bookmarks pane", icon: <BookmarkIcon size={15} />, pressed: shown("bookmarks"), run: () => togglePane("bookmarks") },
    { id: "pane-selection", label: "Selection pane", icon: <Layers size={15} />, pressed: shown("selection"), run: () => togglePane("selection") },
    { id: "pane-sync", label: "Sync slicers pane", icon: <RefreshCw size={15} />, pressed: shown("sync"), run: () => togglePane("sync") },
    { id: "pane-filters", label: "Filters pane", icon: <Filter size={15} />, pressed: shown("filters"), run: () => togglePane("filters") },
    { id: "pane-visualizations", label: "Visualizations pane", icon: <Shapes size={15} />, pressed: shown("visualizations"), run: () => togglePane("visualizations") },
    { id: "pane-fields", label: "Fields pane", icon: <ListTree size={15} />, pressed: shown("fields"), run: () => togglePane("fields") },
    { id: "pane-agent", label: "Agent pane", shortcut: "Ctrl+I", icon: <Sparkles size={15} />, pressed: shown("agent"), run: () => togglePane("agent") },
  ];
  toolsRef.current = tools;


  return (
    <div className={`report-view ${reading ? "is-reading" : ""}`}>
      {formula && <FormulaBar model={model} />}
      <QuickMeasureDialog model={model} />
      <PrintLayout model={model} />
      <ManageRolesDialog model={model} />
      <ThemeEditor model={model} />
      <ViewAsDialog model={model} />
      {reading ? (
        <div className="report-toolbar" role="toolbar" aria-label={t("Reading view")}>
          <span className="report-toolbar-label">{t("Reading view")}</span>
          <span className="status-fill" />
          <Button variant="ghost" size="sm" icon={<Pencil size={14} />} onPress={() => set({ reading: false })}>
            {t("Edit report")}
          </Button>
        </div>
      ) : (
      <div
        className="report-toolbar"
        role="toolbar"
        aria-label={t("Arrange")}
        onContextMenu={(e) => {
          const el = (e.target as HTMLElement).closest<HTMLElement>("[data-command]");
          if (!el) return;
          e.preventDefault();
          const id = el.dataset.command!;
          const tool = tools.find((x) => x.id === id);
          setToolMenu({
            x: e.clientX,
            y: e.clientY,
            label: t("Toolbar"),
            items: [{ id: "hide", label: t("Hide %1 from the toolbar", t(tool?.label ?? id)), run: () => void setLayout((l) => withHidden(l, [id], true)) }],
          });
        }}
      >
        <span className="report-toolbar-label">{chosen.length > 1 ? t("%1 visuals selected", chosen.length) : chosen.length === 1 ? t("1 visual selected") : t("Arrange")}</span>
        {tools.map((x, i) => {
          const sep = x.sep && tools.slice(i + 1).some((y) => !hidden.has(y.id));
          // The second report view has no panes of its own.
          if (hidden.has(x.id) || (!hostsPanes && x.id.startsWith("pane-"))) return x.fill ? <span key={x.id} className="status-fill" /> : null;
          return (
            <Fragment key={x.id}>
              <span className="toolbar-command" data-command={x.id}>
                {x.node ?? <IconButton label={t(x.label)} shortcut={x.shortcut} icon={x.icon} pressed={x.pressed} isDisabled={x.disabled} onPress={x.run} />}
              </span>
              {sep && <span className="topbar-sep" />}
              {x.fill && <span className="status-fill" />}
            </Fragment>
          );
        })}
        {toolMenu && <ContextMenu menu={toolMenu} onClose={() => setToolMenu(null)} />}
      </div>
      )}
      <div
        className={`report-body ${compact ? "is-compact" : ""}`}
        ref={body}
      >
        {!reading && hostsPanes && PANES.filter((k) => placeOf(k) === "left" && (!compact || k === "agent")).map(host)}
        {/* Using a canvas makes its view the one the panes act on (the panes themselves don't). */}
        <div className="report-center" onPointerDownCapture={() => focused !== slot && useReport.getState().set({ slot })}>
          <ViewAsBanner model={model} />
          <DrillBar model={model} page={page} />
          <Canvas model={model} page={page} signature={signature} />
          {focusVisual && <FocusView model={model} page={page} visual={focusVisual} table={!!focus?.table} signature={signature} />}
          <PageTabs model={model} page={page} reading={reading} />
        </div>
        {reading || !hostsPanes ? null : compact ? (
          <>
          {PANES.some(docked) && (
            <div className="pane-stack">
              <div className="pane-switch" role="tablist" aria-label={t("Panes")}>
                {PANES.filter(docked).map((k) => (
                  <button key={k} type="button" role="tab" aria-selected={paneTab === k || (!docked(paneTab) && k === PANES.find(docked))} onClick={() => setPaneTab(k)}>
                    {paneLabel(k)}
                  </button>
                ))}
              </div>
              {renderPane(docked(paneTab) ? paneTab : PANES.find(docked)!)}
            </div>
          )}
          {placeOf("agent") === "right" && host("agent")}
          </>
        ) : (
          PANES.filter((k) => placeOf(k) === "right").map(host)
        )}
        {!reading && hostsPanes && PANES.filter((k) => placeOf(k) === "float").map(host)}
      </div>
    </div>
  );
}

const PANES = PANE_IDS;



interface Tool {
  id: string;
  /** English; translated when shown. */
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  pressed?: boolean;
  disabled?: boolean;
  run?: () => void;
  /** A control of its own (a menu) instead of a button. */
  node?: ReactNode;
  /** A separator after it, and the space that pushes the rest to the end. */
  sep?: boolean;
  fill?: boolean;
}

/** On a page drilled through to: what it shows, and the way back. Leaving the page
 * ends the drill through. */
function DrillBar({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const d = useDrillThrough((s) => s.current);
  useEffect(() => {
    const cur = useDrillThrough.getState().current;
    if (cur && cur.target !== page.id) useDrillThrough.getState().set(null);
  }, [page.id]);
  if (!d || d.target !== page.id) return null;
  const from = model.report.pages.find((p) => p.id === d.from);
  return (
    <div className="drill-bar" role="region" aria-label={t("Drill through")}>
      <IconButton label={from ? t("Back to %1", from.name) : t("Back")} icon={<ArrowLeft size={15} />} onPress={drillBack} />
      <span className="drill-bar-point">
        {d.point.map((p, i) => (
          <span key={i}>
            {i > 0 && ", "}
            {projectionName(model, p.field, t)}: <b>{p.value ?? t("(Blank)")}</b>
          </span>
        ))}
      </span>
    </div>
  );
}
