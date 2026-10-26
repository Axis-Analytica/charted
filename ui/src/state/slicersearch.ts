// A slicer's search, when it has to run in the core: a field with more values than a
// slicer lists (5,000) is searched there, so values past the first 5,000 are found.
// Shorter lists are searched as shown, in the slicer itself. View state, not saved.
import { create } from "zustand";

export const SLICER_LIMIT = 5000;

interface SlicerSearch {
  byVisual: Record<string, string>;
  set: (visual: string, text: string) => void;
}

export const useSlicerSearch = create<SlicerSearch>((set) => ({
  byVisual: {},
  set: (visual, text) =>
    set((s) => {
      if ((s.byVisual[visual] ?? "") === text) return s;
      const byVisual = { ...s.byVisual };
      if (text) byVisual[visual] = text;
      else delete byVisual[visual];
      return { byVisual };
    }),
}));
