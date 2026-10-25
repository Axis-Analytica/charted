import { AlertTriangle, ArrowDownWideNarrow, ArrowUpNarrowWide, Copy, MoreHorizontal, Trash2 } from "lucide-react";
import { type PointerEvent, useContext, useEffect, useRef, useState } from "react";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";
import { useT } from "../i18n";
import { Spinner } from "../kit/misc";
import { useApp } from "../state/app";
import { editVisual, newId, projectionName, useReport, visualTitle } from "../state/report";
import { CanvasScale } from "./scale";
import { useVisualData } from "./data";
import { prop, visualDef } from "./registry";
import { Card } from "./visuals/Card";
import { Cartesian } from "./visuals/Cartesian";
import { TableVisual } from "./visuals/TableVisual";

interface Props {
  visual: Visual;
  page: Page;
  model: Model;
  signature: string;
  selected: boolean;
  register(id: string, el: HTMLElement | null): void;
  onPointerDown(e: PointerEvent): void;
}

export function VisualContainer({ visual, page, model, signature, selected, register, onPointerDown }: Props) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const select = useReport((s) => s.select);
  const ref = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState(false);
  const def = visualDef(visual.visualType);
  const isTable = visual.visualType === "table";
  const { data, error, loading } = useVisualData(visual, page, model, signature, isTable);
  const hasFields = Object.values(visual.roles).some((r) => r && r.length > 0);
  const o = visual.objects;
  const showTitle = prop<boolean>(o, def, "title", "show");
  const bg = prop<boolean>(o, def, "background", "show");
  const border = prop<boolean>(o, def, "background", "border");
  const title = visualTitle(model, visual, t);
  const Icon = def?.icon;

  useEffect(() => {
    register(visual.id, ref.current);
    return () => register(visual.id, null);
  }, [register, visual.id]);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.querySelector(".visual-menu")?.contains(e.target as Node)) setMenu(false);
    };
    window.addEventListener("mousedown", close, true);
    return () => window.removeEventListener("mousedown", close, true);
  }, [menu]);

  const sortable = Object.entries(visual.roles).flatMap(([role, ps]) => (ps ?? []).map((p, index) => ({ role, index, name: projectionName(model, p, t) })));
  const sortBy = (role: string, index: number, descending: boolean) =>
    useApp.getState().edit("Sort visual", editVisual(page.id, visual.id, (v) => ({ ...v, sort: { role, index, descending } })));

  const body = () => {
    if (!def) return <div className="visual-message">{t("Unknown visual type \"%1\"", visual.visualType)}</div>;
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
            onSort={(i) => sortBy("columns", i, visual.sort?.role === "columns" && visual.sort.index === i ? !visual.sort.descending : false)}
          />
        );
      default:
        return <CartesianScaled visual={visual} model={model} data={data} />;
    }
  };

  const p = visual.position;
  return (
    <div
      ref={ref}
      data-visual={visual.id}
      data-testid="visual"
      data-type={visual.visualType}
      role="figure"
      aria-label={title || (def ? t(def.label) : visual.visualType)}
      aria-selected={selected}
      tabIndex={-1}
      className={`visual ${selected ? "is-selected" : ""} ${bg ? "has-bg" : ""} ${bg && border ? "has-border" : ""}`}
      style={{ left: p.x, top: p.y, width: p.width, height: p.height, zIndex: p.z }}
      onPointerDown={onPointerDown}
    >
      {showTitle && title && (
        <div className="visual-title" style={{ fontSize: prop<number>(o, def, "title", "fontSize"), textAlign: prop<"left" | "center" | "right">(o, def, "title", "align") }}>
          {title}
        </div>
      )}
      <div className="visual-body">{body()}</div>
      {loading && hasFields && (
        <span className="visual-loading">
          <Spinner size={12} />
        </span>
      )}
      <div className="visual-header" onPointerDown={(e) => e.stopPropagation()}>
        <button type="button" className="visual-header-btn" aria-label={t("More options")} aria-expanded={menu} onClick={() => setMenu((m) => !m)}>
          <MoreHorizontal size={14} />
        </button>
        {menu && (
          <div className="visual-menu" role="menu">
            {visual.visualType !== "card" &&
              sortable.map((s) => (
                <div key={`${s.role}${s.index}`} className="visual-menu-group">
                  <button type="button" role="menuitem" onClick={() => (sortBy(s.role, s.index, true), setMenu(false))}>
                    <ArrowDownWideNarrow size={13} /> {t("Sort by %1, descending", s.name)}
                  </button>
                  <button type="button" role="menuitem" onClick={() => (sortBy(s.role, s.index, false), setMenu(false))}>
                    <ArrowUpNarrowWide size={13} /> {t("Sort by %1, ascending", s.name)}
                  </button>
                </div>
              ))}
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
    </div>
  );
}

/** Cartesian charts re-create their canvas when the zoom changes, to stay sharp. */
function CartesianScaled(props: Parameters<typeof Cartesian>[0]) {
  const scale = useContext(CanvasScale);
  return <Cartesian key={Math.round(scale * 20)} {...props} />;
}
