// Where a report pane sits: docked left or right (with a resizable edge) or floating over
// the report (dragged by its header, resized from its corner). Right-clicking a pane's
// header moves it; the place is kept in the user's layout (app/layout.ts).
import { type ReactNode, useRef, useState } from "react";
import type { PaneLayout } from "../bridge/generated/PaneLayout";
import { useT } from "../i18n";
import { ContextMenu, type ContextMenuState } from "../kit/ContextMenu";
import { setLayout, withPane } from "../app/layout";

const DEFAULT_FLOAT = { x: 24, y: 24, width: 300, height: 460 };

export function PaneHost({ id, label, pane, children }: { id: string; label: string; pane: PaneLayout; children: ReactNode }) {
  const { t } = useT();
  const ref = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  // A drag or resize in progress, applied to the element directly and saved on release.
  const live = useRef<{ x: number; y: number; width: number; height: number } | null>(null);
  const float = pane.place === "float";

  const save = (patch: Partial<PaneLayout>) => void setLayout((l) => withPane(l, id, patch));
  const openMenu = (x: number, y: number) =>
    setMenu({
      x,
      y,
      label: t("Move %1", label),
      items: [
        { id: "left", label: t("Dock on the left"), disabled: pane.place === "left", run: () => save({ place: "left" }) },
        { id: "right", label: t("Dock on the right"), disabled: pane.place === "right", run: () => save({ place: "right" }) },
        { id: "float", label: t("Float"), disabled: float, run: () => save({ place: "float", float: pane.float ?? DEFAULT_FLOAT }) },
        null,
        { id: "hide", label: t("Hide pane"), run: () => save({ place: "hidden" }) },
      ],
    });

  /** Follows the pointer from `e`, calling `step` with how far it moved, then saves. */
  const track = (e: React.PointerEvent, step: (dx: number, dy: number) => void, done: () => void) => {
    e.preventDefault();
    const sx = e.clientX;
    const sy = e.clientY;
    const move = (m: PointerEvent) => step(m.clientX - sx, m.clientY - sy);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      done();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const startMove = (e: React.PointerEvent) => {
    const head = (e.target as HTMLElement).closest(".pane-head");
    if (!float || !head || (e.target as HTMLElement).closest("button, input, select, [role=button]") || e.button !== 0) return;
    const start = { ...(pane.float ?? DEFAULT_FLOAT) };
    const parent = ref.current?.parentElement?.getBoundingClientRect();
    track(
      e,
      (dx, dy) => {
        const x = Math.max(0, Math.min(start.x + dx, (parent?.width ?? 2000) - 80));
        const y = Math.max(0, Math.min(start.y + dy, (parent?.height ?? 2000) - 34));
        live.current = { ...start, x, y };
        if (ref.current) Object.assign(ref.current.style, { left: `${x}px`, top: `${y}px` });
      },
      () => live.current && (save({ float: live.current }), (live.current = null)),
    );
  };

  const startResize = (e: React.PointerEvent) => {
    if (float) {
      const start = { ...(pane.float ?? DEFAULT_FLOAT) };
      track(
        e,
        (dx, dy) => {
          live.current = { ...start, width: Math.max(180, start.width + dx), height: Math.max(120, start.height + dy) };
          if (ref.current) Object.assign(ref.current.style, { width: `${live.current.width}px`, height: `${live.current.height}px` });
        },
        () => live.current && (save({ float: live.current }), (live.current = null)),
      );
      return;
    }
    const width = ref.current?.getBoundingClientRect().width ?? 240;
    // A pane docked right grows as the edge moves left (mirrored right to left).
    const rtl = document.documentElement.dir === "rtl";
    const sign = (pane.place === "right") !== rtl ? -1 : 1;
    let w = width;
    track(
      e,
      (dx) => {
        w = Math.max(180, Math.min(640, width + sign * dx));
        ref.current?.style.setProperty("--pane-w", `${w}px`);
      },
      () => save({ width: w }),
    );
  };

  const f = pane.float ?? DEFAULT_FLOAT;
  return (
    <div
      ref={ref}
      className={`pane-host is-${pane.place} ${pane.width && !float ? "has-width" : ""}`}
      data-pane={id}
      style={float ? { left: f.x, top: f.y, width: f.width, height: f.height } : pane.width ? ({ "--pane-w": `${pane.width}px` } as React.CSSProperties) : undefined}
      onPointerDown={startMove}
      onContextMenu={(e) => {
        if (!(e.target as HTMLElement).closest(".pane-head")) return;
        e.preventDefault();
        openMenu(e.clientX, e.clientY);
      }}
      onKeyDown={(e) => {
        // Shift+F10 or the Menu key on the header opens the same menu.
        if ((e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) && (e.target as HTMLElement).closest(".pane-head")) {
          e.preventDefault();
          const r = (e.target as HTMLElement).getBoundingClientRect();
          openMenu(r.left, r.bottom);
        }
      }}
    >
      {children}
      <div className={`pane-resize ${float ? "is-corner" : ""}`} aria-hidden onPointerDown={startResize} />
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
