import {
  AlertTriangle,
  ArrowDown,
  ArrowDownWideNarrow,
  ArrowUp,
  ArrowUpNarrowWide,
  Ban,
  ChevronsDownUp,
  ChevronsUpDown,
  Copy,
  Download,
  Filter,
  Focus,
  ChevronsDown,
  CornerDownRight,
  GitFork,
  Highlighter,
  ImagePlus,
  MoreHorizontal,
  Sheet,
  Trash2,
} from "lucide-react";
import { type PointerEvent, useContext, useEffect, useMemo, useRef, useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { InteractionKind } from "../bridge/generated/InteractionKind";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Projection } from "../bridge/generated/Projection";
import type { Visual } from "../bridge/generated/Visual";
import { msg, useLanguage, useT } from "../i18n";
import { Spinner } from "../kit/misc";
import { useApp } from "../state/app";
import { drillInto, drillNext, drillUp, expandDown, hierarchy, useDrillMode } from "../state/drill";
import { drillTargets, drillThrough } from "../state/drillthrough";
import { editVisual, isGrouping, newId, pageSettings, projectionName, useReport, visualTitle } from "../state/report";
import { interaction, interactionKinds, isDataVisual, selectionEffect, useSelection } from "../state/selection";
import { toast } from "../state/toasts";
import { type VisualData, useVisualData } from "./data";
import { prop, visualDef } from "./registry";
import { CanvasScale } from "./scale";
import { VisualBoundary } from "./VisualBoundary";
import { Card } from "./visuals/Card";
import { fmt, projectionFormat } from "./visuals/common";
import { Cartesian } from "./visuals/Cartesian";
import { ImageVisual, placeImage, Shape, TextBox } from "./visuals/Decor";
import type { Interact } from "./visuals/interact";
import { Matrix, shapeMatrix, useExpansion } from "./visuals/Matrix";
import { Pie } from "./visuals/Pie";
import { Scatter } from "./visuals/Scatter";
import { Slicer } from "./visuals/Slicer";
import { TableVisual } from "./visuals/TableVisual";

interface Props {
  visual: Visual;
  page: Page;
  model: Model;
  signature: string;
  selected: boolean;
  register(id: string, el: HTMLElement | null): void;
  onPointerDown(e: PointerEvent): void;
  /** Shown outside the page (focus mode): fills its container instead of its position. */
  standalone?: boolean;
}

const CARTESIAN = ["columnChart", "barChart", "lineChart", "stackedColumnChart", "stackedBarChart"];

/** The visual's own choice of points, as the renderers use it. */
type PointMenu = { fields: Projection[]; tuple: (string | null)[]; x: number; y: number };

function useInteract(visual: Visual, page: Page, onMenu: (m: PointMenu) => void): Interact {
  const selection = useSelection((s) => s.selection);
  const drillMode = useDrillMode((s) => s.on.includes(visual.id));
  const canDown = !!hierarchy(visual)?.canDown;
  return useMemo(() => {
    const mine = selection && selection.source === visual.id && selection.page === page.id ? selection : null;
    return {
      drill: drillMode && canDown ? (keys) => void drillInto(page.id, visual, keys) : undefined,
      menu: (fields, tuple, at) => onMenu({ fields, tuple, ...at }),
      choose: (fields, tuple, additive) => useSelection.getState().choose(page.id, visual.id, fields, tuple, additive),
      clear: () => {
        if (useSelection.getState().selection?.source === visual.id) useSelection.getState().clear();
      },
      chosen: (tuple) => (mine ? mine.tuples.some((t) => t.length === tuple.length && t.every((x, i) => x === tuple[i])) : null),
    };
  }, [selection, visual, page.id, drillMode, canDown, onMenu]);
}

export function VisualContainer({ visual, page, model, signature, selected, register, onPointerDown, standalone }: Props) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const select = useReport((s) => s.select);
  const setReport = useReport((s) => s.set);
  const editingInteractions = useReport((s) => s.editInteractions);
  const chosenVisuals = useReport((s) => s.selected);
  const ref = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState(false);
  const [pointMenu, setPointMenu] = useState<PointMenu | null>(null);
  const def = visualDef(visual.visualType);
  const isTable = visual.visualType === "table";
  const decor = !!def?.decor;
  const { data, error, loading } = useVisualData(visual, page, model, signature, isTable);
  const interact = useInteract(visual, page, setPointMenu);
  const hasFields = Object.values(visual.roles).some((r) => r && r.length > 0);
  const o = visual.objects;
  const showTitle = prop<boolean>(o, def, "title", "show");
  const bg = prop<boolean>(o, def, "background", "show");
  const border = prop<boolean>(o, def, "background", "border");
  const title = def?.objects.some((x) => x.name === "title") ? visualTitle(model, visual, t) : "";
  const Icon = def?.icon;
  // The source of the interactions being edited: the one selected visual.
  const source = editingInteractions && chosenVisuals.length === 1 && chosenVisuals[0] !== visual.id ? page.visuals.find((v) => v.id === chosenVisuals[0]) : undefined;
  const drill = hierarchy(visual);
  const drillable = !!drill && drill.fields.length > 1;
  const drillMode = useDrillMode((s) => s.on.includes(visual.id));
  const crossFiltered = useSelection((s) => {
    const e = selectionEffect(s.selection, page, visual);
    return e.filters.length > 0;
  });

  useEffect(() => {
    if (standalone) return;
    register(visual.id, ref.current);
    return () => register(visual.id, null);
  }, [register, visual.id, standalone]);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.querySelector(".visual-menu")?.contains(e.target as Node)) setMenu(false);
    };
    window.addEventListener("mousedown", close, true);
    return () => window.removeEventListener("mousedown", close, true);
  }, [menu]);

  const sortable = isDataVisual(visual.visualType) && !["card", "slicer", "matrix"].includes(visual.visualType) ? Object.entries(visual.roles).flatMap(([role, ps]) => (ps ?? []).map((p, index) => ({ role, index, name: projectionName(model, p, t) }))) : [];
  const sortBy = (role: string, index: number, descending: boolean) =>
    useApp.getState().edit("Sort visual", editVisual(page.id, visual.id, (v) => ({ ...v, sort: { role, index, descending } })));

  const exportData = async () => {
    const name = (title || t(def?.label ?? "Visual")).replace(/[/\\?%*:|"<>]/g, "-");
    const path = await transport.pick({ title: t("Export data"), saveAs: `${name}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (!path) return;
    const filters = selectionEffect(useSelection.getState().selection, page, visual).filters;
    try {
      await transport.rpc({ type: "exportVisualCsv", visual: visual.id, path, filters });
      toast({ kind: "success", title: t("Exported the data"), detail: path });
    } catch (e) {
      toast({ kind: "error", title: errorMessage(e) });
    }
  };

  const body = () => {
    if (!def) return <div className="visual-message">{t("Unknown visual type \"%1\"", visual.visualType)}</div>;
    switch (visual.visualType) {
      case "textbox":
        return <TextBox visual={visual} page={page} />;
      case "image":
        return <ImageVisual visual={visual} page={page} />;
      case "shape":
        return <Shape visual={visual} />;
    }
    if (!hasFields)
      return (
        <div className="visual-placeholder" aria-hidden>
          {Icon && <Icon size={Math.min(64, Math.max(28, visual.position.height / 4))} strokeWidth={1.25} />}
        </div>
      );
    if (error)
      return (
        <div className="visual-message is-error" role="alert">
          <AlertTriangle size={18} />
          <span>{t("Couldn't display the visual")}</span>
          <span className="visual-error-detail">{error}</span>
        </div>
      );
    if (visual.visualType === "slicer") return <Slicer visual={visual} page={page} model={model} data={data} />;
    if (!data) return null;
    if (data.rows === 0 && visual.visualType !== "card") return <div className="visual-message">{t("No data")}</div>;
    switch (visual.visualType) {
      case "card":
        return <Card visual={visual} model={model} data={data} />;
      case "table":
        return (
          <TableVisual
            visual={visual}
            model={model}
            data={data}
            interact={interact}
            onSort={(i) => sortBy("columns", i, visual.sort?.role === "columns" && visual.sort.index === i ? !visual.sort.descending : false)}
          />
        );
      case "matrix":
        return <Matrix visual={visual} model={model} data={data} interact={interact} />;
      case "pieChart":
      case "donutChart":
        return <Scaled render={(k) => <Pie key={k} visual={visual} model={model} data={data} interact={interact} />} />;
      case "scatterChart":
        return <Scaled render={(k) => <Scatter key={k} visual={visual} model={model} data={data} interact={interact} />} />;
      default:
        return CARTESIAN.includes(visual.visualType) ? <Scaled render={(k) => <Cartesian key={k} visual={visual} model={model} data={data} interact={interact} />} /> : null;
    }
  };

  const p = visual.position;
  const style = standalone ? undefined : { left: p.x, top: p.y, width: p.width, height: p.height, zIndex: p.z };
  return (
    <div
      ref={ref}
      data-visual={standalone ? undefined : visual.id}
      data-focus-visual={standalone ? visual.id : undefined}
      data-testid="visual"
      data-type={visual.visualType}
      role="figure"
      aria-label={title || (def ? t(def.label) : visual.visualType)}
      aria-selected={standalone ? undefined : selected}
      tabIndex={-1}
      className={`visual ${standalone ? "is-standalone" : ""} ${selected && !standalone ? "is-selected" : ""} ${bg ? "has-bg" : ""} ${bg && border ? "has-border" : ""}`}
      style={style}
      onPointerDown={standalone ? undefined : onPointerDown}
    >
      {showTitle && title && (
        <div className="visual-title" style={{ fontSize: prop<number>(o, def, "title", "fontSize"), textAlign: prop<"left" | "center" | "right">(o, def, "title", "align") }}>
          {title}
        </div>
      )}
      <div className="visual-body">
        <VisualBoundary resetKey={`${signature}:${JSON.stringify(visual)}`}>{body()}</VisualBoundary>
      </div>
      {loading && hasFields && (
        <span className="visual-loading">
          <Spinner size={12} />
        </span>
      )}
      {source && isDataVisual(visual.visualType) && isDataVisual(source.visualType) && <InteractionPicker page={page} source={source} target={visual} />}
      {pointMenu && <PointMenuView menu={pointMenu} visual={visual} page={page} model={model} onClose={() => setPointMenu(null)} />}
      {!standalone && (
        <div className="visual-header" onPointerDown={(e) => e.stopPropagation()}>
          {crossFiltered && (
            <span className="visual-badge" title={t("Filtered by a selection in another visual")} aria-label={t("Filtered by a selection in another visual")}>
              <Filter size={11} />
            </span>
          )}
          {drillable && drill && (
            <>
              <button type="button" className="visual-header-btn" aria-label={t("Drill up")} title={t("Drill up")} disabled={!drill.canUp} onClick={() => drillUp(page.id, visual)}>
                <ArrowUp size={13} />
              </button>
              <button
                type="button"
                className={`visual-header-btn ${drillMode ? "is-on" : ""}`}
                aria-label={t("Drill down mode")}
                title={drillMode ? t("Drill down mode is on: click a data point to drill into it") : t("Turn on drill down mode")}
                aria-pressed={drillMode}
                disabled={!drill.canDown && !drillMode}
                onClick={() => useDrillMode.getState().toggle(visual.id)}
              >
                <ArrowDown size={13} />
              </button>
              <button type="button" className="visual-header-btn" aria-label={t("Go to the next level in the hierarchy")} title={t("Go to the next level in the hierarchy")} disabled={!drill.canDown} onClick={() => drillNext(page.id, visual)}>
                <ChevronsDown size={13} />
              </button>
              <button type="button" className="visual-header-btn" aria-label={t("Expand all down one level in the hierarchy")} title={t("Expand all down one level in the hierarchy")} disabled={!drill.canDown} onClick={() => expandDown(page.id, visual)}>
                <GitFork size={13} />
              </button>
            </>
          )}
          {!decor && hasFields && (
            <button type="button" className="visual-header-btn" aria-label={t("Focus mode")} title={t("Focus mode")} onClick={() => setReport({ focus: { visual: visual.id, table: false } })}>
              <Focus size={13} />
            </button>
          )}
          <button type="button" className="visual-header-btn" aria-label={t("More options")} aria-expanded={menu} onClick={() => setMenu((m) => !m)}>
            <MoreHorizontal size={14} />
          </button>
          {menu && (
            <div className="visual-menu" role="menu">
              {sortable.length > 0 && (
                <div className="visual-menu-group">
                  {sortable.map((s) => (
                    <div key={`${s.role}${s.index}`} className="visual-menu-pair">
                      <button type="button" role="menuitem" onClick={() => (sortBy(s.role, s.index, true), setMenu(false))}>
                        <ArrowDownWideNarrow size={13} /> {t("Sort by %1, descending", s.name)}
                      </button>
                      <button type="button" role="menuitem" onClick={() => (sortBy(s.role, s.index, false), setMenu(false))}>
                        <ArrowUpNarrowWide size={13} /> {t("Sort by %1, ascending", s.name)}
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {visual.visualType === "matrix" && data && (
                <div className="visual-menu-group">
                  <button
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      setMenu(false);
                      useExpansion.getState().setAll(visual.id, shapeMatrix(visual, model, data, "", "en").parents);
                    }}
                  >
                    <ChevronsUpDown size={13} /> {t("Expand all")}
                  </button>
                  <button type="button" role="menuitem" onClick={() => (setMenu(false), useExpansion.getState().setAll(visual.id, []))}>
                    <ChevronsDownUp size={13} /> {t("Collapse all")}
                  </button>
                </div>
              )}
              {!decor && hasFields && (
                <div className="visual-menu-group">
                  <button type="button" role="menuitem" onClick={() => (setMenu(false), setReport({ focus: { visual: visual.id, table: true } }))}>
                    <Sheet size={13} /> {t("Show as a table")}
                  </button>
                  <button type="button" role="menuitem" onClick={() => (setMenu(false), exportData())}>
                    <Download size={13} /> {t("Export data")}
                  </button>
                </div>
              )}
              {visual.visualType === "image" && (
                <button type="button" role="menuitem" onClick={() => (setMenu(false), placeImage(page, visual, t("Choose an image")))}>
                  <ImagePlus size={13} /> {t("Change image…")}
                </button>
              )}
              <button
                type="button"
                role="menuitem"
                onClick={async () => {
                  setMenu(false);
                  const copy: Visual = { ...visual, id: newId(), position: { ...p, x: Math.min(page.width - p.width, p.x + 16), y: Math.min(page.height - p.height, p.y + 16), z: p.z + 1 } };
                  if (await commit("Duplicate visual", [{ op: "addVisual", page: page.id, visual: copy, index: null }])) select([copy.id]);
                }}
              >
                <Copy size={13} /> {t("Duplicate")}
              </button>
              <button type="button" role="menuitem" className="is-danger" onClick={() => (setMenu(false), commit("Remove visual", [{ op: "removeVisual", page: page.id, visual: visual.id }]))}>
                <Trash2 size={13} /> {t("Remove")}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** A data point's context menu: drill through to pages set up for its fields, drill
 * down into it, drill up. */
function PointMenuView({ menu, visual, page, model, onClose }: { menu: PointMenu; visual: Visual; page: Page; model: Model; onClose(): void }) {
  const { t } = useT();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
    const close = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close, true);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("keydown", key, true);
    };
  }, [onClose]);
  const targets = drillTargets(model, page, menu.fields);
  const h = hierarchy(visual);
  // The point's values at the hierarchy levels shown.
  const levelKeys = h ? h.shown.map((i) => menu.tuple[menu.fields.findIndex((f) => f === h.fields[i])] ?? null) : [];
  const item = (label: string, run: () => void, icon?: React.ReactNode) => (
    <button
      key={label}
      type="button"
      role="menuitem"
      onClick={() => {
        onClose();
        run();
      }}
    >
      {icon}
      {label}
    </button>
  );
  return (
    <div ref={ref} className="visual-menu point-menu" role="menu" aria-label={t("Data point")} style={{ position: "fixed", left: menu.x, top: menu.y }} onPointerDown={(e) => e.stopPropagation()}>
      {h && h.canDown && item(t("Drill down"), () => drillInto(page.id, visual, levelKeys), <ArrowDown size={13} />)}
      {h && h.canUp && item(t("Drill up"), () => drillUp(page.id, visual), <ArrowUp size={13} />)}
      {targets.length > 0 && (
        <div className="visual-menu-group" role="group" aria-label={t("Drill through")}>
          <div className="visual-menu-label">{t("Drill through")}</div>
          {targets.map((p) => item(p.name, () => drillThrough(page, visual, menu.fields, menu.tuple, p), <CornerDownRight size={13} />))}
        </div>
      )}
      {!targets.length && !(h && (h.canDown || h.canUp)) && <div className="visual-menu-label">{t("No drill-through pages for this data")}</div>}
    </div>
  );
}

const KIND_ICON: Record<InteractionKind, typeof Filter> = { filter: Filter, highlight: Highlighter, none: Ban };
const KIND_LABEL: Record<InteractionKind, string> = { filter: msg("Filter"), highlight: msg("Highlight"), none: msg("None") };

/** Edit interactions: how a selection in `source` affects this visual. */
function InteractionPicker({ page, source, target }: { page: Page; source: Visual; target: Visual }) {
  const { t } = useT();
  const current = interaction(page, source.id, target.id);
  const set = (kind: InteractionKind) =>
    useApp.getState().edit("Edit interaction", (model) => {
      const p = model.report.pages.find((x) => x.id === page.id);
      if (!p) return null;
      const others = (p.interactions ?? []).filter((i) => !(i.source === source.id && i.target === target.id));
      // Store only what differs from the default.
      const fallback = interaction({ ...p, interactions: [] }, source.id, target.id);
      const interactions = kind === fallback ? others : [...others, { source: source.id, target: target.id, kind }];
      return [{ op: "setPage", page: p.id, settings: { ...pageSettings(p), interactions } }];
    });
  return (
    <div className="interaction-picker" role="radiogroup" aria-label={t("Interaction with %1", visualTitle(useApp.getState().state!.project!.model, source, t) || t("the selected visual"))} onPointerDown={(e) => e.stopPropagation()}>
      {interactionKinds(target).map((k) => {
        const I = KIND_ICON[k];
        return (
          <button key={k} type="button" role="radio" aria-checked={current === k} className={current === k ? "is-on" : ""} title={t(KIND_LABEL[k])} aria-label={t(KIND_LABEL[k])} onClick={() => set(k)}>
            <I size={13} />
          </button>
        );
      })}
    </div>
  );
}

/** Canvas charts re-create their canvas when the zoom changes, to stay sharp. */
function Scaled({ render }: { render(key: number): React.ReactNode }) {
  const scale = useContext(CanvasScale);
  return <>{render(Math.round(scale * 20))}</>;
}

/** A visual's data as a plain table (Show as a table). */
export function DataTable({ visual, model, data }: { visual: Visual; model: Model; data: VisualData }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const columns = Object.entries(visual.roles).flatMap(([role, ps]) => (ps ?? []).map((p, i) => ({ name: `${role}:${i}`, caption: projectionName(model, p, t), format: projectionFormat(model, p), group: isGrouping(p) })));
  const present = columns.filter((c) => data.columns.has(c.name));
  // A matrix sends every level; the table shows the most detailed one.
  const level = data.columns.get("level:rows");
  const levelCols = data.columns.get("level:columns");
  let rows = Array.from({ length: data.rows }, (_, r) => r);
  if (level && levelCols) {
    const deepest = (v: typeof level) => rows.reduce((m, r) => Math.max(m, Number(v.get(r))), 0);
    const dr = deepest(level);
    const dc = deepest(levelCols);
    rows = rows.filter((r) => Number(level.get(r)) === dr && Number(levelCols.get(r)) === dc);
  }
  // Values align right; groups (even years) read as labels.
  const numeric = (c: (typeof present)[number]) => !c.group && ["Int64", "Double", "Decimal"].includes(c.format.type);
  return (
    <div className="table-visual data-table has-lines">
      <table>
        <thead>
          <tr>
            {present.map((c) => (
              <th key={c.name} className={numeric(c) ? "is-num" : ""}>
                {c.caption}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.slice(0, 5000).map((r) => (
            <tr key={r}>
              {present.map((c) => (
                <td key={c.name} className={numeric(c) ? "is-num tabular" : ""}>
                  {fmt(data.columns.get(c.name)?.get(r), c.format, locale)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
