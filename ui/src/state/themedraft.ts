// The theme being edited in the theme editor, shown on the canvas as a live preview
// (view state). `undefined` when nobody is editing; `null` previews following the
// desktop. OK commits it as one undoable step; Cancel drops it.
import { create } from "zustand";
import type { ReportTheme } from "../bridge/generated/ReportTheme";

export const useThemeDraft = create<{ draft: ReportTheme | null | undefined; set(draft: ReportTheme | null | undefined): void }>((set) => ({
  draft: undefined,
  set: (draft) => set({ draft }),
}));
