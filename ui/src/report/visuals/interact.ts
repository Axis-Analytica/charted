// What a visual renderer can do with its own data points: choose them (which filters or
// highlights the rest of the page) and ask whether one is chosen (to dim the others).
import type { Projection } from "../../bridge/generated/Projection";

export interface Interact {
  /** Chooses a point given by its group values; `additive` (Ctrl) adds or removes it. */
  choose(fields: Projection[], tuple: (string | null)[], additive: boolean): void;
  /** Clears this visual's choice (a click on empty space). */
  clear(): void;
  /** Whether a point is chosen: null when nothing in this visual is. */
  chosen(tuple: (string | null)[]): boolean | null;
}

export const noInteract: Interact = { choose() {}, clear() {}, chosen: () => null };

/** Whether a pointer event asks to add to the selection. */
export const additive = (e: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean } | undefined) => !!(e && (e.ctrlKey || e.metaKey));
