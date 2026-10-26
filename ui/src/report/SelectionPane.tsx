// The Selection pane: the page's visuals as layers, top first. Clicking one selects it
// (Ctrl or Shift adds it), the eye hides or shows it, and dragging (or Alt+↑/↓) moves
// it up or down the layer order, which is also the tab order (`position.z`). Hidden
// visuals are not drawn on the page but can still be selected and formatted here.
import { ChevronDown, ChevronUp, Eye, EyeOff, GripVertical } from "lucide-react";
import { useRef, useState } from "react";
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";
import { useT } from "../i18n";
import { IconButton } from "../kit/Button";
import { useApp } from "../state/app";
import { updateVisual, useReport, visualTitle } from "../state/report";
import { visualDef } from "./registry";
import "./building.css";

const LAYER_MIME = "application/x-charted-layer";

/** The page's visuals top layer first (ties: later in the list is on top). */
export function layers(page: Page): Visual[] {
  return page.visuals.map((v, i) => ({ v, i })).sort((a, b) => b.v.position.z - a.v.position.z || b.i - a.i).map((x) => x.v);
}

/** Changes that give the visuals, listed top first, the layer order 0..n (top highest). */
export function relayer(page: string, top: Visual[]): Change[] {
  const n = top.length;
  return top.flatMap((v, i) => (v.position.z === n - 1 - i ? [] : [updateVisual(page, { ...v, position: { ...v.position, z: n - 1 - i } })]));
}

/** A visual's name in the pane: its title, else its type. */
export function layerName(model: Model, v: Visual, t: (s: string, ...a: (string | number)[]) => string): string {
  const def = visualDef(v.visualType);
  const title = def?.objects.some((o) => o.name === "title") ? visualTitle(model, v, t) : "";
  const text = v.visualType === "textbox" ? String((v.objects?.text as Record<string, unknown> | undefined)?.content ?? "").split("\n")[0] : "";
  const button = v.visualType === "actionButton" ? String((v.objects?.button as Record<string, unknown> | undefined)?.text ?? "") : "";
  return title || (text ?? "").trim() || button.trim() || (def ? t(def.label) : v.visualType);
}

export function SelectionPane({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const edit = useApp((s) => s.edit);
  const selected = useReport((s) => s.selected);
  const select = useReport((s) => s.select);
  const list = useRef<HTMLUListElement>(null);
  const [over, setOver] = useState<number | null>(null);
  const top = layers(page);

  const setHidden = (ids: string[], hidden: boolean) =>
    edit(hidden ? "Hide visual" : "Show visual", (m) => {
      const p = m.report.pages.find((x) => x.id === page.id);
      return p?.visuals.filter((v) => ids.includes(v.id) && !!v.hidden !== hidden).map((v) => updateVisual(p.id, { ...v, hidden }));
    });
  /** Moves `ids` so the first of them lands at `to` in the top-first list. */
  const moveTo = (ids: string[], to: number) =>
    edit("Change layer order", (m) => {
      const p = m.report.pages.find((x) => x.id === page.id);
      if (!p) return null;
      const order = layers(p);
      const moving = order.filter((v) => ids.includes(v.id));
      const rest = order.filter((v) => !ids.includes(v.id));
      const before = order.slice(0, to).filter((v) => !ids.includes(v.id)).length;
      rest.splice(before, 0, ...moving);
      return relayer(p.id, rest);
    });
  const step = (ids: string[], by: -1 | 1) => {
    const idx = top.map((v, i) => (ids.includes(v.id) ? i : -1)).filter((i) => i >= 0);
    if (!idx.length) return;
    const first = Math.min(...idx);
    const last = Math.max(...idx);
    if ((by < 0 && first === 0) || (by > 0 && last === top.length - 1)) return;
    // Up: in front of the layer above the first; down: behind the layer below the last.
    void moveTo(ids, by < 0 ? first - 1 : last + 2);
  };
  const focusLayer = (id: string) => requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-layer="${id}"] .layer-name`)?.focus());
  const focusAt = (i: number) => requestAnimationFrame(() => list.current?.querySelectorAll<HTMLElement>(".layer-name")[i]?.focus());
  const chosen = selected.filter((id) => page.visuals.some((v) => v.id === id));

  return (
    <section className="pane selection-pane" aria-label={t("Selection")}>
      <div className="pane-head">
        <span>{t("Selection")}</span>
        <span className="pane-head-tools">
          <IconButton label={t("Bring forward")} icon={<ChevronUp size={14} />} isDisabled={!chosen.length} onPress={() => step(chosen, -1)} />
          <IconButton label={t("Send backward")} icon={<ChevronDown size={14} />} isDisabled={!chosen.length} onPress={() => step(chosen, 1)} />
        </span>
      </div>
      <div className="pane-body">
        {top.length === 0 ? (
          <p className="pane-hint">{t("This page has no visuals.")}</p>
        ) : (
          <>
            <div className="layer-actions">
              <button type="button" className="link-button" onClick={() => setHidden(page.visuals.map((v) => v.id), false)}>
                {t("Show all")}
              </button>
              <button type="button" className="link-button" onClick={() => setHidden(page.visuals.map((v) => v.id), true)}>
                {t("Hide all")}
              </button>
            </div>
            <ul ref={list} className="layer-list" aria-label={t("Layer order")}>
              {top.map((v, i) => {
                const name = layerName(model, v, t);
                const on = selected.includes(v.id);
                return (
                  <li
                    key={v.id}
                    className={`layer ${on ? "is-on" : ""} ${v.hidden ? "is-hidden" : ""} ${over === i ? "is-over" : ""}`}
                    data-layer={v.id}
                    draggable
                    onDragStart={(e) => {
                      e.dataTransfer.setData(LAYER_MIME, JSON.stringify(on ? chosen : [v.id]));
                      e.dataTransfer.effectAllowed = "move";
                    }}
                    onDragOver={(e) => {
                      if (!e.dataTransfer.types.includes(LAYER_MIME)) return;
                      e.preventDefault();
                      setOver(i);
                    }}
                    onDragLeave={() => setOver((o) => (o === i ? null : o))}
                    onDrop={(e) => {
                      setOver(null);
                      const raw = e.dataTransfer.getData(LAYER_MIME);
                      if (!raw) return;
                      e.preventDefault();
                      void moveTo(JSON.parse(raw) as string[], i);
                    }}
                  >
                    <GripVertical size={12} className="layer-grip" />
                    <button
                      type="button"
                      className="layer-name"
                      aria-pressed={on}
                      title={name}
                      onClick={(e) => {
                        if (e.ctrlKey || e.metaKey || e.shiftKey) select(on ? selected.filter((x) => x !== v.id) : [...selected, v.id]);
                        else select([v.id]);
                      }}
                      onKeyDown={(e) => {
                        if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
                          e.preventDefault();
                          const by = e.key === "ArrowUp" ? -1 : 1;
                          const to = i + by;
                          if (to < 0 || to >= top.length) return;
                          void moveTo([v.id], by < 0 ? to : to + 1).then(() => focusLayer(v.id));
                        } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
                          e.preventDefault();
                          focusAt(i + (e.key === "ArrowDown" ? 1 : -1));
                        }
                      }}
                    >
                      <span>{name}</span>
                    </button>
                    <button
                      type="button"
                      className="layer-eye"
                      aria-label={v.hidden ? t("Show %1", name) : t("Hide %1", name)}
                      title={v.hidden ? t("Show") : t("Hide")}
                      aria-pressed={!!v.hidden}
                      onClick={() => setHidden([v.id], !v.hidden)}
                    >
                      {v.hidden ? <EyeOff size={14} /> : <Eye size={14} />}
                    </button>
                  </li>
                );
              })}
            </ul>
            <p className="pane-hint">{t("The top layer is drawn in front and comes first in the tab order. Drag layers, or press Alt+↑ and Alt+↓, to reorder them.")}</p>
          </>
        )}
      </div>
    </section>
  );
}
