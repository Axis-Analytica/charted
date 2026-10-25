// The report page: visuals at absolute positions in page units, scaled to fit. Moveable
// drags and resizes (snapping to an 8px grid and to other visuals' edges and centres);
// Selecto draws the marquee. Positions are committed once, when a gesture ends.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import Moveable from "react-moveable";
import Selecto from "react-selecto";
import type { Change } from "../bridge/generated/Change";
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Position } from "../bridge/generated/Position";
import type { Visual } from "../bridge/generated/Visual";
import { useT } from "../i18n";
import { useApp } from "../state/app";
import { newVisual, updateVisual, useReport, withField } from "../state/report";
import { useSelection } from "../state/selection";
import { CanvasScale, ThemeKey } from "./scale";
import { themeStyle, themeTokens } from "./themes";
import { VisualContainer } from "./VisualContainer";

export const FIELD_MIME = "application/x-charted-field";
export const VISUAL_MIME = "application/x-charted-visual";


const GRID = 8;

/** The report theme's tokens for page content, or null to follow the desktop. */
export function usePageTheme(model: Model) {
  const desktop = useApp((s) => s.env?.desktop);
  const theme = model.report.theme ?? null;
  const tokens = useMemo(() => (desktop ? themeTokens(theme, desktop) : null), [theme, desktop]);
  const key = useMemo(() => JSON.stringify(tokens), [tokens]);
  return { tokens, key, style: themeStyle(tokens) };
}

export function Canvas({ model, page, signature }: { model: Model; page: Page; signature: string }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const selected = useReport((s) => s.selected);
  const select = useReport((s) => s.select);
  const setReport = useReport((s) => s.set);
  const scroller = useRef<HTMLDivElement>(null);
  const pageEl = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);
  const [els, setEls] = useState<Map<string, HTMLElement>>(new Map());
  const moveable = useRef<Moveable>(null);
  // Live offsets while dragging/resizing, by visual id.
  const live = useRef(new Map<string, Position>());

  // Fit the page into the available space.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const fit = () => {
      const s = Math.min((el.clientWidth - 32) / page.width, (el.clientHeight - 32) / page.height);
      setScale(Math.max(0.2, Math.min(4, Math.floor(s * 100) / 100)));
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, [page.width, page.height]);

  const register = useCallback((id: string, el: HTMLElement | null) => {
    setEls((m) => {
      if (el ? m.get(id) === el : !m.has(id)) return m;
      const n = new Map(m);
      if (el) n.set(id, el);
      else n.delete(id);
      return n;
    });
  }, []);

  const byId = useMemo(() => new Map(page.visuals.map((v) => [v.id, v])), [page.visuals]);
  // The report theme's tokens on the page (none: the page follows the desktop theme).
  const { tokens, key: themeKey, style: pageStyle } = usePageTheme(model);
  const targets = selected.map((id) => els.get(id)).filter((e): e is HTMLElement => !!e);
  const guidelines = page.visuals.filter((v) => !selected.includes(v.id)).map((v) => els.get(v.id)).filter((e): e is HTMLElement => !!e);

  /** Page coordinates of a pointer event. */
  const toPage = (clientX: number, clientY: number) => {
    const r = pageEl.current!.getBoundingClientRect();
    return { x: (clientX - r.left) / scale, y: (clientY - r.top) / scale };
  };

  const commitPositions = (label: string) => {
    const changes: Change[] = [];
    for (const [id, pos] of live.current) {
      const v = byId.get(id);
      if (!v) continue;
      const width = Math.round(Math.min(pos.width, page.width));
      const height = Math.round(Math.min(pos.height, page.height));
      // Keep visuals on the page.
      const x = Math.round(Math.max(0, Math.min(page.width - width, pos.x)));
      const y = Math.round(Math.max(0, Math.min(page.height - height, pos.y)));
      const position = { ...v.position, x, y, width, height };
      if (JSON.stringify(position) !== JSON.stringify(v.position)) changes.push(updateVisual(page.id, { ...v, position }));
    }
    live.current.clear();
    if (changes.length) commit(label, changes);
  };

  const start = (id: string) => {
    const v = byId.get(id);
    if (v) live.current.set(id, { ...v.position });
  };

  const onDropField = async (field: FieldRef, at: { x: number; y: number }, target?: Visual) => {
    if (target) {
      const next = withField(model, target, field);
      if (next) await commit("Add field", [updateVisual(page.id, next)]);
      select([target.id]);
      return;
    }
    const v = newVisual(page, field.kind === "measure" ? "card" : "table", at);
    const filled = withField(model, v, field) ?? v;
    if (await commit("Add visual", [{ op: "addVisual", page: page.id, visual: filled, index: null }])) select([v.id]);
  };

  // Positions change through the model (commits, undo); move the handles with them.
  useLayoutEffect(() => {
    moveable.current?.updateRect();
  }, [page.visuals, scale, selected]);

  // Keep the selection valid when visuals go away (undo, delete).
  useEffect(() => {
    const valid = selected.filter((id) => byId.has(id));
    if (valid.length !== selected.length) select(valid);
  }, [byId, selected, select]);

  return (
    <CanvasScale.Provider value={scale}>
      <ThemeKey.Provider value={themeKey}>
      <div
        ref={scroller}
        className="canvas-scroller"
        tabIndex={-1}
        onPointerDown={(e) => {
          if (e.target === e.currentTarget) {
            select([]);
            useSelection.getState().clear();
          }
        }}
      >
        <div className="page-frame" style={{ width: page.width * scale, height: page.height * scale }}>
          <div
            ref={pageEl}
            className="page"
            data-testid="report-page"
            aria-label={t("Page %1", page.name)}
            data-themed={tokens ? "" : undefined}
            style={{ ...pageStyle, width: page.width, height: page.height, transform: `scale(${scale})` }}
            onPointerMove={(e) => setReport({ pointer: toPage(e.clientX, e.clientY) })}
            onPointerDown={(e) => {
              if (e.target === e.currentTarget && !e.shiftKey && !e.ctrlKey) {
                select([]);
                useSelection.getState().clear();
              }
            }}
            onDragOver={(e) => {
              if (e.dataTransfer.types.includes(FIELD_MIME) || e.dataTransfer.types.includes(VISUAL_MIME)) {
                e.preventDefault();
                e.dataTransfer.dropEffect = "copy";
              }
            }}
            onDrop={(e) => {
              const at = toPage(e.clientX, e.clientY);
              const field = e.dataTransfer.getData(FIELD_MIME);
              const type = e.dataTransfer.getData(VISUAL_MIME);
              const hit = (e.target as HTMLElement).closest<HTMLElement>("[data-visual]");
              const target = hit ? byId.get(hit.dataset.visual!) : undefined;
              if (field) {
                e.preventDefault();
                onDropField(JSON.parse(field) as FieldRef, at, target);
              } else if (type) {
                e.preventDefault();
                const v = newVisual(page, type, at);
                commit("Add visual", [{ op: "addVisual", page: page.id, visual: v, index: null }]).then((ok) => ok && select([v.id]));
              }
            }}
          >
            {page.visuals.map((v) => (
              <VisualContainer
                key={v.id}
                visual={v}
                page={page}
                model={model}
                signature={signature}
                selected={selected.includes(v.id)}
                register={register}
                onPointerDown={(e) => {
                  if (e.shiftKey || e.ctrlKey || e.metaKey) {
                    select(selected.includes(v.id) ? selected.filter((x) => x !== v.id) : [...selected, v.id]);
                  } else if (!selected.includes(v.id)) select([v.id]);
                }}
              />
            ))}
            {page.visuals.length === 0 && (
              <div className="page-empty">
                <p>{t("Build visuals with your data")}</p>
                <p className="page-empty-hint">{t("Select or drag fields from the Fields pane onto the report canvas.")}</p>
              </div>
            )}
            <Moveable
              ref={moveable}
              target={targets}
              draggable
              resizable={targets.length === 1}
              origin={false}
              keepRatio={false}
              throttleDrag={0}
              throttleResize={0}
              snappable
              snapGridWidth={GRID}
              snapGridHeight={GRID}
              snapThreshold={6}
              isDisplaySnapDigit={false}
              snapDirections={{ left: true, top: true, right: true, bottom: true, center: true, middle: true }}
              elementSnapDirections={{ left: true, top: true, right: true, bottom: true, center: true, middle: true }}
              elementGuidelines={guidelines}
              renderDirections={["nw", "n", "ne", "w", "e", "sw", "s", "se"]}
              className="moveable-control"
              zoom={1 / scale}
              onDragStart={(e) => start(e.target.getAttribute("data-visual")!)}
              onDrag={(e) => {
                const id = e.target.getAttribute("data-visual")!;
                const v = byId.get(id);
                if (!v) return;
                const [dx = 0, dy = 0] = e.beforeTranslate;
                e.target.style.transform = `translate(${dx}px, ${dy}px)`;
                live.current.set(id, { ...v.position, x: v.position.x + dx, y: v.position.y + dy });
              }}
              onDragEnd={(e) => {
                e.target.style.transform = "";
                commitPositions("Move visual");
              }}
              onDragGroupStart={(e) => e.events.forEach((ev) => start(ev.target.getAttribute("data-visual")!))}
              onDragGroup={(e) =>
                e.events.forEach((ev) => {
                  const id = ev.target.getAttribute("data-visual")!;
                  const v = byId.get(id);
                  if (!v) return;
                  const [dx = 0, dy = 0] = ev.beforeTranslate;
                  ev.target.style.transform = `translate(${dx}px, ${dy}px)`;
                  live.current.set(id, { ...v.position, x: v.position.x + dx, y: v.position.y + dy });
                })
              }
              onDragGroupEnd={(e) => {
                e.targets.forEach((el) => ((el as HTMLElement).style.transform = ""));
                commitPositions("Move visuals");
              }}
              onResizeStart={(e) => {
                start(e.target.getAttribute("data-visual")!);
                e.setMin([64, 48]);
              }}
              onResize={(e) => {
                const id = e.target.getAttribute("data-visual")!;
                const v = byId.get(id);
                if (!v) return;
                e.target.style.width = `${e.width}px`;
                e.target.style.height = `${e.height}px`;
                const [dx = 0, dy = 0] = e.drag.beforeTranslate;
                e.target.style.transform = `translate(${dx}px, ${dy}px)`;
                live.current.set(id, { ...v.position, x: v.position.x + dx, y: v.position.y + dy, width: e.width, height: e.height });
              }}
              onResizeEnd={(e) => {
                e.target.style.transform = "";
                commitPositions("Resize visual");
              }}
            />
          </div>
          <Selecto
            container={scroller.current}
            dragContainer={scroller.current}
            selectableTargets={[".visual"]}
            hitRate={0}
            selectByClick={false}
            selectFromInside={false}
            toggleContinueSelect={["shift"]}
            ratio={0}
            dragCondition={(e) => {
              const t = e.inputEvent.target as HTMLElement;
              return !t.closest(".visual") && !t.closest(".moveable-control");
            }}
            onSelectEnd={(e) => {
              const ids = e.selected.map((el) => el.getAttribute("data-visual")!).filter(Boolean);
              select(e.inputEvent.shiftKey ? [...new Set([...selected, ...ids])] : ids);
            }}
          />
        </div>
      </div>
      </ThemeKey.Provider>
    </CanvasScale.Provider>
  );
}
