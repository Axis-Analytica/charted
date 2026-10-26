// The format painter: one visual's look copied onto others (see paintFormat).
import { create } from "zustand";
import type { Visual } from "../bridge/generated/Visual";
import { useApp } from "../state/app";
import { updateVisual } from "../state/report";
import { paintFormat } from "./formatting";

type Look = Pick<Visual, "visualType" | "objects">;

/** The format painter: the copied look; whether the next visual clicked takes it
 * (`armed`, from the toolbar button), and whether it stays on after that (`sticky`,
 * a double-click). Ctrl+Alt+C copies a look without arming; Ctrl+Alt+V pastes it. */
export const usePainter = create<{ source: Look | null; armed: boolean; sticky: boolean; set(p: Partial<{ source: Look | null; armed: boolean; sticky: boolean }>): void }>((set) => ({
  source: null,
  armed: false,
  sticky: false,
  set: (p) => set(p),
}));

/** Paints the copied look onto visuals on `page` (one undo step). */
export function paint(page: string, ids: string[]) {
  const { source, sticky } = usePainter.getState();
  if (!source || !ids.length) return Promise.resolve(false);
  if (!sticky) usePainter.getState().set({ armed: false });
  return useApp.getState().edit(ids.length > 1 ? "Paste formatting" : "Paint formatting", (m) => {
    const p = m.report.pages.find((x) => x.id === page);
    return p?.visuals.filter((v) => ids.includes(v.id)).flatMap((v) => {
      const next = paintFormat(source, v);
      return JSON.stringify(next) === JSON.stringify(v) ? [] : [updateVisual(p.id, next)];
    });
  });
}
