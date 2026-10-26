import {
  ArrowLeft,
  Bookmark as BookmarkIcon,
  Check,
  MousePointerClick,
  Palette,
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
  ListTree,
  Plus,
  SendToBack,
  Shapes,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";
import { useShortcut } from "../app/shortcuts";
import { useT } from "../i18n";
import { IconButton } from "../kit/Button";
import { useApp } from "../state/app";
import { currentPage, newId, newVisual, pageSettings, updateVisual, useReport } from "../state/report";
import { Canvas, usePageTheme } from "./Canvas";
import { modelSignature, useVisualData } from "./data";
import { CanvasScale, ThemeKey } from "./scale";
import { BUILT_IN } from "./themes";
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
import { VISUALS } from "./registry";
import { VisualizationsPane } from "./VisualizationsPane";
import "./report.css";

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

function PageTabs({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const set = useReport((s) => s.set);
  const [renaming, setRenaming] = useState<string | null>(null);
  const settings = pageSettings;
  const addPage = async () => {
    const names = new Set(model.report.pages.map((p) => p.name.toLowerCase()));
    let n = model.report.pages.length + 1;
    while (names.has(t("Page %1", n).toLowerCase())) n++;
    const id = newId();
    const p: Page = { id, name: t("Page %1", n), width: page.width, height: page.height, filters: [], interactions: [], visuals: [] };
    if (await commit("Add page", [{ op: "addPage", page: p, index: null }])) set({ page: id, selected: [] });
  };
  return (
    <nav className="page-tabs" aria-label={t("Pages")}>
      <div role="tablist" className="page-tab-list">
        {model.report.pages.map((p, i) => (
          <div key={p.id} className={`page-tab ${p.id === page.id ? "is-on" : ""}`}>
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
                onDoubleClick={() => setRenaming(p.id)}
                onKeyDown={(e) => {
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
              </button>
            )}
          </div>
        ))}
      </div>
      <IconButton label={t("New page")} icon={<Plus size={14} />} onPress={addPage} />
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
  const { page: pageId, selected, panes, formula, set, select, editInteractions, focus } = useReport();
  const page = currentPage(model, pageId);
  const signature = useMemo(() => modelSignature(model), [model]);
  const pageRef = useRef(page);
  pageRef.current = page;
  // In narrow windows (a tiled half screen) the three panes share one tabbed column.
  const body = useRef<HTMLDivElement>(null);
  const [compact, setCompact] = useState(false);
  const [paneTab, setPaneTab] = useState<"bookmarks" | "filters" | "visualizations" | "fields">("visualizations");
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setCompact(el.clientWidth < 1240));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    if (page && page.id !== pageId) set({ page: page.id });
  }, [page, pageId, set]);
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
  const hasSelection = () => useReport.getState().selected.length > 0 && useReport.getState().view === "report" && canvasFocused();
  const zOrder = (front: boolean) => {
    if (!page) return;
    const zs = page.visuals.map((v) => v.position.z);
    const edge = front ? Math.max(...zs) + 1 : Math.min(...zs) - 1;
    apply(front ? "Bring to front" : "Send to back", chosen.map((v, i) => ({ ...v, position: { ...v.position, z: edge + (front ? i : -i) } })));
  };

  useBookmarkShortcuts(model);
  useShortcut(
    { id: "visual-delete", keys: "Delete", label: "Delete selected visuals", group: "Edit", run: () => page && commit("Remove visuals", useReport.getState().selected.map((id): Change => ({ op: "removeVisual", page: page.id, visual: id }))), enabled: hasSelection },
    { id: "visual-left", keys: "ArrowLeft", label: "Nudge left", group: "Edit", run: () => nudge(-1, 0), enabled: hasSelection },
    { id: "visual-right", keys: "ArrowRight", label: "Nudge right", group: "Edit", run: () => nudge(1, 0), enabled: hasSelection },
    { id: "visual-up", keys: "ArrowUp", label: "Nudge up", group: "Edit", run: () => nudge(0, -1), enabled: hasSelection },
    { id: "visual-down", keys: "ArrowDown", label: "Nudge down", group: "Edit", run: () => nudge(0, 1), enabled: hasSelection },
    { id: "visual-left8", keys: "Shift+ArrowLeft", label: "Move left by 8", group: "Edit", run: () => nudge(-8, 0), enabled: hasSelection },
    { id: "visual-right8", keys: "Shift+ArrowRight", label: "Move right by 8", group: "Edit", run: () => nudge(8, 0), enabled: hasSelection },
    { id: "visual-up8", keys: "Shift+ArrowUp", label: "Move up by 8", group: "Edit", run: () => nudge(0, -8), enabled: hasSelection },
    { id: "visual-down8", keys: "Shift+ArrowDown", label: "Move down by 8", group: "Edit", run: () => nudge(0, 8), enabled: hasSelection },
    { id: "visual-all", keys: "Ctrl+A", label: "Select all visuals", group: "Edit", run: () => page && select(page.visuals.map((v) => v.id)), enabled: () => useReport.getState().view === "report" && canvasFocused() },
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
      enabled: () => useReport.getState().view === "report" && canvasFocused() && (useReport.getState().selected.length > 0 || !!useSelection.getState().selection || !!useReport.getState().focus),
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
      enabled: () => useReport.getState().view === "report" && !!page,
    })),
  );

  if (!page) return <div className="main-empty">{t("This report has no pages.")}</div>;
  const align = (how: Align | "distributeH" | "distributeV") => apply(how.startsWith("distribute") ? "Distribute visuals" : "Align visuals", arrange(chosen, how));
  const togglePane = (k: keyof typeof panes) => set({ panes: { ...panes, [k]: !panes[k] } });

  return (
    <div className="report-view">
      {formula && <FormulaBar model={model} />}
      <QuickMeasureDialog model={model} />
      <div className="report-toolbar" role="toolbar" aria-label={t("Arrange")}>
        <span className="report-toolbar-label">{chosen.length > 1 ? t("%1 visuals selected", chosen.length) : chosen.length === 1 ? t("1 visual selected") : t("Arrange")}</span>
        <IconButton label={t("Align left")} icon={<AlignStartVertical size={15} />} isDisabled={chosen.length < 2} onPress={() => align("left")} />
        <IconButton label={t("Align center")} icon={<AlignCenterVertical size={15} />} isDisabled={chosen.length < 2} onPress={() => align("center")} />
        <IconButton label={t("Align right")} icon={<AlignEndVertical size={15} />} isDisabled={chosen.length < 2} onPress={() => align("right")} />
        <IconButton label={t("Align top")} icon={<AlignStartHorizontal size={15} />} isDisabled={chosen.length < 2} onPress={() => align("top")} />
        <IconButton label={t("Align middle")} icon={<AlignCenterHorizontal size={15} />} isDisabled={chosen.length < 2} onPress={() => align("middle")} />
        <IconButton label={t("Align bottom")} icon={<AlignEndHorizontal size={15} />} isDisabled={chosen.length < 2} onPress={() => align("bottom")} />
        <IconButton label={t("Distribute horizontally")} icon={<AlignHorizontalDistributeCenter size={15} />} isDisabled={chosen.length < 3} onPress={() => align("distributeH")} />
        <IconButton label={t("Distribute vertically")} icon={<AlignVerticalDistributeCenter size={15} />} isDisabled={chosen.length < 3} onPress={() => align("distributeV")} />
        <span className="topbar-sep" />
        <IconButton label={t("Bring to front")} icon={<BringToFront size={15} />} isDisabled={!chosen.length} onPress={() => zOrder(true)} />
        <IconButton label={t("Send to back")} icon={<SendToBack size={15} />} isDisabled={!chosen.length} onPress={() => zOrder(false)} />
        <span className="topbar-sep" />
        <IconButton label={t("Edit interactions")} icon={<MousePointerClick size={15} />} pressed={editInteractions} onPress={() => set({ editInteractions: !editInteractions })} />
        <ThemeMenu model={model} />
        <span className="status-fill" />
        <IconButton label={t("Bookmarks pane")} icon={<BookmarkIcon size={15} />} pressed={panes.bookmarks} onPress={() => togglePane("bookmarks")} />
        <IconButton label={t("Filters pane")} icon={<Filter size={15} />} pressed={panes.filters} onPress={() => togglePane("filters")} />
        <IconButton label={t("Visualizations pane")} icon={<Shapes size={15} />} pressed={panes.visualizations} onPress={() => togglePane("visualizations")} />
        <IconButton label={t("Fields pane")} icon={<ListTree size={15} />} pressed={panes.fields} onPress={() => togglePane("fields")} />
      </div>
      <div className={`report-body ${compact ? "is-compact" : ""}`} ref={body}>
        <div className="report-center">
          <DrillBar model={model} page={page} />
          <Canvas model={model} page={page} signature={signature} />
          {focusVisual && <FocusView model={model} page={page} visual={focusVisual} table={!!focus?.table} signature={signature} />}
          <PageTabs model={model} page={page} />
        </div>
        {compact ? (
          (panes.bookmarks || panes.filters || panes.visualizations || panes.fields) && (
            <div className="pane-stack">
              <div className="pane-switch" role="tablist" aria-label={t("Panes")}>
                {PANES.filter((k) => panes[k]).map((k) => (
                  <button key={k} type="button" role="tab" aria-selected={paneTab === k || (!panes[paneTab] && k === PANES.find((x) => panes[x]))} onClick={() => setPaneTab(k)}>
                    {k === "bookmarks" ? t("Bookmarks") : k === "filters" ? t("Filters") : k === "visualizations" ? t("Visualizations") : t("Fields")}
                  </button>
                ))}
              </div>
              {(() => {
                const shown = panes[paneTab] ? paneTab : PANES.find((x) => panes[x]);
                if (shown === "bookmarks") return <BookmarksPane model={model} page={page} />;
                if (shown === "filters") return <FiltersPane model={model} page={page} />;
                if (shown === "visualizations") return <VisualizationsPane model={model} page={page} />;
                return <FieldsPane model={model} page={page} />;
              })()}
            </div>
          )
        ) : (
          <>
            {panes.bookmarks && <BookmarksPane model={model} page={page} />}
            {panes.filters && <FiltersPane model={model} page={page} />}
            {panes.visualizations && <VisualizationsPane model={model} page={page} />}
            {panes.fields && <FieldsPane model={model} page={page} />}
          </>
        )}
      </div>
    </div>
  );
}

const PANES = ["bookmarks", "filters", "visualizations", "fields"] as const;

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
