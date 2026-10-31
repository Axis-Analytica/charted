import { CalendarDays, KeyRound, LayoutGrid, Maximize, Minus, MoreHorizontal, Plus, Link2 } from "lucide-react";
import { type PointerEvent as ReactPointerEvent, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { DiagramPosition } from "../bridge/generated/DiagramPosition";
import type { Model } from "../bridge/generated/Model";
import type { Relationship } from "../bridge/generated/Relationship";
import type { Table } from "../bridge/generated/Table";
import { useShortcut } from "../app/shortcuts";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Menu } from "../kit/Menu";
import { ColumnIcon, MeasureIcon, TableIcon } from "../views/icons";
import { useApp } from "../state/app";
import { newId } from "../state/report";
import { arrow, CARD_W, cardHeight, edge, type Point } from "./geometry";
import { layout, type LayoutResult } from "./layout";
import { DateTableDialog } from "./DateTableDialog";
import { RelationshipDialog } from "./RelationshipDialog";
import "./model.css";
import { Hideable } from "../app/Hideable";

type Drag = { kind: "card"; table: string; start: Point; origin: Point } | { kind: "pan"; start: Point; origin: Point } | { kind: "link"; table: string; column: string; from: Point; to: Point };

/** A plain grid, used if the layout worker cannot run. */
function gridLayout(model: Model): LayoutResult {
  const out: LayoutResult = {};
  model.tables.forEach((t, i) => (out[t.id] = { x: 40 + (i % 4) * (CARD_W + 80), y: 40 + Math.floor(i / 4) * 420 }));
  return out;
}

/** Lays tables out as a star: fact tables in the middle, the tables that filter them
 * split between the left and the right. ELK places layers left to right, so edges to
 * right-hand tables are reversed. */
function autoLayout(model: Model): Promise<LayoutResult> {
  const facts = new Set(model.relationships.filter((r) => r.fromCardinality === "Many" || r.toCardinality === "Many").map((r) => (r.toCardinality === "Many" ? r.toTable : r.fromTable)));
  const degree = (id: string) => model.relationships.filter((r) => r.fromTable === id || r.toTable === id).length;
  const dims = model.tables.filter((t) => !facts.has(t.id)).sort((a, b) => degree(b.id) - degree(a.id));
  const right = new Set(dims.filter((_, i) => i % 2 === 1).map((t) => t.id));
  return layout({
    nodes: model.tables.map((t) => ({ id: t.id, width: CARD_W, height: cardHeight(t) })),
    edges: model.relationships.map((r) => {
      // Filters flow from the one side to the many side.
      const [one, many] = r.toCardinality === "Many" ? [r.fromTable, r.toTable] : [r.toTable, r.fromTable];
      return right.has(one) ? { id: r.id, source: many, target: one } : { id: r.id, source: one, target: many };
    }),
  }).catch((e) => {
    console.error("model layout failed:", e);
    return gridLayout(model);
  });
}

export function relationshipLabel(model: Model, r: Relationship): string {
  const name = (t: string, c: string) => {
    const tbl = model.tables.find((x) => x.id === t);
    return `${tbl?.name ?? "?"}[${tbl?.columns.find((x) => x.id === c)?.name ?? "?"}]`;
  };
  return `${name(r.fromTable, r.fromColumn)} → ${name(r.toTable, r.toColumn)}`;
}

export function ModelView({ model }: { model: Model }) {
  const { t, tn } = useT();
  const edit = useApp((s) => s.edit);
  const [auto, setAuto] = useState<LayoutResult | null>(null);
  const [moving, setMoving] = useState<{ table: string; at: Point } | null>(null);
  const [view, setView] = useState({ x: 0, y: 0, zoom: 1 });
  const [selected, setSelected] = useState<string | null>(null);
  const [dialog, setDialog] = useState<{ r: Relationship; isNew: boolean } | null>(null);
  const [dateTable, setDateTable] = useState(false);
  const [drag, setDrag] = useState<Drag | null>(null);
  const canvas = useRef<HTMLDivElement>(null);
  const fitted = useRef(false);

  const stored = model.report.diagram;
  const missing = model.tables.some((x) => !stored[x.id]);
  // Re-run the automatic layout only when tables come or go, so cards stay put.
  const tableKey = model.tables.map((x) => x.id).join(",");
  useEffect(() => {
    if (!missing) return;
    let live = true;
    autoLayout(model).then((r) => live && setAuto(r));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [missing, tableKey]);

  const pos = useMemo(() => {
    const m = new Map<string, Point>();
    for (const tbl of model.tables) {
      const p = stored[tbl.id] ?? auto?.[tbl.id];
      if (p) m.set(tbl.id, { x: p.x, y: p.y });
    }
    if (moving) m.set(moving.table, moving.at);
    return m;
  }, [model.tables, stored, auto, moving]);
  const tables = useMemo(() => new Map(model.tables.map((x) => [x.id, x])), [model.tables]);
  const ready = pos.size === model.tables.length;

  const fit = () => {
    const el = canvas.current;
    if (!el || !pos.size) return;
    let x0 = Infinity;
    let y0 = Infinity;
    let x1 = -Infinity;
    let y1 = -Infinity;
    for (const [id, p] of pos) {
      const tbl = tables.get(id);
      if (!tbl) continue;
      x0 = Math.min(x0, p.x);
      y0 = Math.min(y0, p.y);
      x1 = Math.max(x1, p.x + CARD_W);
      y1 = Math.max(y1, p.y + cardHeight(tbl));
    }
    const w = el.clientWidth;
    const h = el.clientHeight;
    const zoom = Math.max(0.3, Math.min(1.25, Math.min((w - 64) / (x1 - x0), (h - 64) / (y1 - y0))));
    setView({ zoom, x: (w - (x1 - x0) * zoom) / 2 - x0 * zoom, y: (h - (y1 - y0) * zoom) / 2 - y0 * zoom });
  };
  useLayoutEffect(() => {
    if (ready && !fitted.current) {
      fitted.current = true;
      fit();
    }
  });

  /** Canvas pixel → diagram coordinates. */
  const toPlane = (e: { clientX: number; clientY: number }): Point => {
    const r = canvas.current!.getBoundingClientRect();
    return { x: (e.clientX - r.left - view.x) / view.zoom, y: (e.clientY - r.top - view.y) / view.zoom };
  };

  const savePositions = (table: string, at: Point) => {
    const all: Record<string, DiagramPosition> = {};
    for (const [id, p] of pos) all[id] = { x: Math.round(p.x), y: Math.round(p.y) };
    all[table] = { x: Math.round(at.x), y: Math.round(at.y) };
    edit("Move table", () => [{ op: "setDiagram", positions: all }]).finally(() => setMoving(null));
  };
  const resetLayout = () => {
    fitted.current = false;
    setAuto(null);
    edit("Arrange tables", () => [{ op: "setDiagram", positions: {} }]);
  };

  const newRelationship = (from?: { table: string; column: string }, to?: { table: string; column: string }) => {
    const ft = from?.table ?? model.tables[0]?.id ?? "";
    const tt = to?.table ?? model.tables.find((x) => x.id !== ft)?.id ?? ft;
    const col = (id: string) => model.tables.find((x) => x.id === id)?.columns[0]?.id ?? "";
    setDialog({
      isNew: true,
      r: {
        id: newId(),
        fromTable: ft,
        fromColumn: from?.column ?? col(ft),
        toTable: tt,
        toColumn: to?.column ?? col(tt),
        fromCardinality: "Many",
        toCardinality: "One",
        crossFilter: "Single",
        active: !model.relationships.some((r) => r.active && ((r.fromTable === ft && r.toTable === tt) || (r.fromTable === tt && r.toTable === ft))),
      },
    });
  };
  const selectedRel = model.relationships.find((r) => r.id === selected);
  useShortcut(
    { id: "model-new-relationship", keys: "", label: "New relationship…", group: "Edit", run: () => newRelationship() },
    { id: "model-edit-relationship", keys: "Enter", label: "Edit relationship", group: "Edit", run: () => selectedRel && setDialog({ r: selectedRel, isNew: false }), enabled: () => !!selectedRel },
    { id: "model-delete-relationship", keys: "Delete", label: "Delete relationship", group: "Edit", run: () => selectedRel && edit("Delete relationship", () => [{ op: "removeRelationship", relationship: selectedRel.id }]), enabled: () => !!selectedRel },
    { id: "model-fit", keys: "Ctrl+0", label: "Fit diagram", group: "View", run: fit },
    { id: "model-date-table", keys: "", label: "New date table", group: "Edit", run: () => setDateTable(true) },
    { id: "model-arrange", keys: "", label: "Arrange the diagram", group: "View", run: () => resetLayout() },
    { id: "model-zoom-in", keys: "", label: "Zoom in", group: "View", run: () => zoomBy(1.2) },
    { id: "model-zoom-out", keys: "", label: "Zoom out", group: "View", run: () => zoomBy(1 / 1.2) },
  );

  const onPointerMove = (e: ReactPointerEvent) => {
    if (!drag) return;
    if (drag.kind === "pan") setView((v) => ({ ...v, x: drag.origin.x + e.clientX - drag.start.x, y: drag.origin.y + e.clientY - drag.start.y }));
    if (drag.kind === "card") {
      const p = toPlane(e);
      setMoving({ table: drag.table, at: { x: drag.origin.x + p.x - drag.start.x, y: drag.origin.y + p.y - drag.start.y } });
    }
    if (drag.kind === "link") setDrag({ ...drag, to: toPlane(e) });
  };
  const onPointerUp = (e: ReactPointerEvent) => {
    if (!drag) return;
    setDrag(null);
    if (drag.kind === "card") {
      if (moving && (moving.at.x !== drag.origin.x || moving.at.y !== drag.origin.y)) savePositions(drag.table, moving.at);
      else setMoving(null);
    }
    if (drag.kind === "link") {
      const target = (document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null)?.closest<HTMLElement>("[data-column]");
      const table = target?.dataset.table;
      const column = target?.dataset.column;
      if (table && column && table !== drag.table) newRelationship({ table: drag.table, column: drag.column }, { table, column });
    }
  };
  const onWheel = (e: React.WheelEvent) => {
    if (e.ctrlKey || e.metaKey) {
      const r = canvas.current!.getBoundingClientRect();
      const cx = e.clientX - r.left;
      const cy = e.clientY - r.top;
      setView((v) => {
        const zoom = Math.max(0.25, Math.min(2, v.zoom * Math.exp(-e.deltaY * 0.0015)));
        return { zoom, x: cx - ((cx - v.x) * zoom) / v.zoom, y: cy - ((cy - v.y) * zoom) / v.zoom };
      });
    } else {
      setView((v) => ({ ...v, x: v.x - e.deltaX, y: v.y - e.deltaY }));
    }
  };
  const zoomBy = (f: number) => {
    const el = canvas.current;
    if (!el) return;
    const cx = el.clientWidth / 2;
    const cy = el.clientHeight / 2;
    setView((v) => {
      const zoom = Math.max(0.25, Math.min(2, v.zoom * f));
      return { zoom, x: cx - ((cx - v.x) * zoom) / v.zoom, y: cy - ((cy - v.y) * zoom) / v.zoom };
    });
  };

  const linkStart = (e: ReactPointerEvent, tbl: Table, column: string) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    canvas.current?.setPointerCapture(e.pointerId);
    const p = toPlane(e);
    setDrag({ kind: "link", table: tbl.id, column, from: p, to: p });
  };

  return (
    <div className="model-view">
      <div className="model-toolbar">
        <Hideable id="model-new-relationship">
          <Button size="sm" icon={<Link2 size={14} />} onPress={() => newRelationship()}>
            {t("New relationship")}
          </Button>
        </Hideable>
        <Hideable id="model-date-table">
          <Button size="sm" variant="ghost" icon={<CalendarDays size={14} />} onPress={() => setDateTable(true)}>
            {t("New date table")}
          </Button>
        </Hideable>
        <Hideable id="model-arrange">
          <Button size="sm" variant="ghost" icon={<LayoutGrid size={14} />} onPress={resetLayout}>
            {t("Arrange")}
          </Button>
        </Hideable>
        <span className="model-toolbar-gap" />
        <Hideable id="model-zoom">
          <IconButton label={t("Zoom out")} icon={<Minus size={15} />} onPress={() => zoomBy(1 / 1.2)} />
          <span className="model-zoom tabular">{Math.round(view.zoom * 100)}%</span>
          <IconButton label={t("Zoom in")} icon={<Plus size={15} />} onPress={() => zoomBy(1.2)} />
        </Hideable>
        <Hideable id="model-fit">
          <IconButton label={t("Fit diagram")} shortcut="Ctrl+0" icon={<Maximize size={15} />} onPress={fit} />
        </Hideable>
      </div>
      <div className="model-body">
        <div
          ref={canvas}
          className={`model-canvas ${drag?.kind === "pan" ? "is-panning" : ""} ${drag?.kind === "link" ? "is-linking" : ""}`}
          onPointerDown={(e) => {
            if (e.button !== 0 && e.button !== 1) return;
            // No text selection or native drag: they would cancel the pointer stream.
            e.preventDefault();
            setSelected(null);
            canvas.current?.setPointerCapture(e.pointerId);
            setDrag({ kind: "pan", start: { x: e.clientX, y: e.clientY }, origin: { x: view.x, y: view.y } });
          }}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={() => {
            setDrag(null);
            setMoving(null);
          }}
          onWheel={onWheel}
        >
          <div className="model-plane" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})` }}>
            <svg className="model-edges" width="1" height="1" overflow="visible" aria-hidden>
              {ready &&
                model.relationships.map((r) => {
                  const g = edge(r, tables, pos);
                  if (!g) return null;
                  const on = selected === r.id;
                  // Filters flow from the one side to the many side; "Both" adds the reverse.
                  const toward = r.fromCardinality === "One" && r.toCardinality === "Many" ? g.tangent : { x: -g.tangent.x, y: -g.tangent.y };
                  const back = { x: -toward.x, y: -toward.y };
                  const both = r.crossFilter === "Both";
                  const off = (d: Point, k: number) => ({ x: g.mid.x + d.x * k, y: g.mid.y + d.y * k });
                  const mark = (c: string, p: Point & { dir: number }) => (
                    <text x={p.x + p.dir * 12} y={p.y - 5} textAnchor="middle" className="model-card-mark">
                      {c === "One" ? "1" : "*"}
                    </text>
                  );
                  return (
                    <g
                      key={r.id}
                      className={`model-edge ${on ? "is-selected" : ""} ${r.active ? "" : "is-inactive"}`}
                      data-relationship={r.id}
                      onPointerDown={(e) => {
                        e.stopPropagation();
                        setSelected(r.id);
                      }}
                      onDoubleClick={() => setDialog({ r, isNew: false })}
                    >
                      <path d={g.path} className="model-edge-hit" />
                      <path d={g.path} className="model-edge-line" />
                      {both ? (
                        <>
                          <path d={arrow(off(toward, 7), toward)} className="model-edge-arrow" />
                          <path d={arrow(off(back, 7), back)} className="model-edge-arrow" />
                        </>
                      ) : (
                        <path d={arrow(g.mid, toward)} className="model-edge-arrow" />
                      )}
                      {mark(r.fromCardinality, g.from)}
                      {mark(r.toCardinality, g.to)}
                    </g>
                  );
                })}
              {drag?.kind === "link" && <line x1={drag.from.x} y1={drag.from.y} x2={drag.to.x} y2={drag.to.y} className="model-link-preview" />}
            </svg>
            {ready &&
              model.tables.map((tbl) => {
                const p = pos.get(tbl.id)!;
                return (
                  <section
                    key={tbl.id}
                    className={`model-card ${moving?.table === tbl.id ? "is-moving" : ""}`}
                    style={{ left: p.x, top: p.y, width: CARD_W, height: cardHeight(tbl) }}
                    aria-label={tbl.name}
                    data-table-card={tbl.id}
                  >
                    <header
                      className="model-card-head"
                      onPointerDown={(e) => {
                        if (e.button !== 0) return;
                        e.stopPropagation();
                        e.preventDefault();
                        canvas.current?.setPointerCapture(e.pointerId);
                        setDrag({ kind: "card", table: tbl.id, start: toPlane(e), origin: p });
                      }}
                    >
                      <span className="model-card-title">{tbl.name}</span>
                      {tbl.calculated && (
                        <span className="model-card-badge">
                          <TableIcon table={tbl} size={13} />
                        </span>
                      )}
                      {tbl.isDateTable && (
                        <span className="model-card-badge" title={t("Date table")}>
                          <CalendarDays size={13} />
                        </span>
                      )}
                      <span onPointerDown={(e) => e.stopPropagation()}>
                        <Menu
                          label={t("Table options")}
                          trigger={<MoreHorizontal size={14} />}
                          items={[
                            {
                              id: "date",
                              label: tbl.isDateTable ? t("Unmark as date table") : t("Mark as date table"),
                              icon: <CalendarDays size={14} />,
                              run: () => edit(tbl.isDateTable ? "Unmark as date table" : "Mark as date table", () => [{ op: "setDateTable", table: tbl.id, isDateTable: !tbl.isDateTable }]),
                            },
                          ]}
                        />
                      </span>
                    </header>
                    <ul className="model-card-cols">
                      {tbl.columns.map((c) => (
                        <li
                          key={c.id}
                          className={`model-col ${c.hidden ? "is-hidden" : ""}`}
                          data-table={tbl.id}
                          data-column={c.id}
                          title={t("Drag onto a column in another table to relate them")}
                          onPointerDown={(e) => linkStart(e, tbl, c.id)}
                        >
                          <span className="model-col-icon">{c.isKey ? <KeyRound size={13} /> : <ColumnIcon column={c} size={13} />}</span>
                          <span className="model-col-name">{c.name}</span>
                        </li>
                      ))}
                      {tbl.measures.length > 0 && (
                        <li className="model-col model-col-measures">
                          <span className="model-col-icon">
                            <MeasureIcon size={13} />
                          </span>
                          <span className="model-col-name">{tn("%n measures", tbl.measures.length, "%n measure")}</span>
                        </li>
                      )}
                    </ul>
                  </section>
                );
              })}
          </div>
        </div>
        <aside className="model-side" aria-label={t("Relationships")}>
          <div className="sidebar-head">
            <span>{t("Relationships")}</span>
            <span className="sidebar-count">{model.relationships.length}</span>
          </div>
          <ul className="model-rel-list">
            {model.relationships.map((r) => (
              <li key={r.id}>
                <button
                  type="button"
                  className={`model-rel ${selected === r.id ? "is-selected" : ""}`}
                  onClick={() => setSelected(r.id)}
                  onDoubleClick={() => setDialog({ r, isNew: false })}
                  onKeyDown={(e) => e.key === "Enter" && (e.stopPropagation(), setDialog({ r, isNew: false }))}
                >
                  <span className="model-rel-name">{relationshipLabel(model, r)}</span>
                  <span className="model-rel-meta">
                    {r.fromCardinality === "One" ? "1" : "*"}:{r.toCardinality === "One" ? "1" : "*"}
                    {r.crossFilter === "Both" ? ` · ${t("Both")}` : ""}
                    {r.active ? "" : ` · ${t("Inactive")}`}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </aside>
      </div>
      {dateTable && <DateTableDialog model={model} onClose={() => setDateTable(false)} />}
      {dialog && <RelationshipDialog model={model} initial={dialog.r} isNew={dialog.isNew} onClose={() => setDialog(null)} />}
    </div>
  );
}
