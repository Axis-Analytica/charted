// Which of two states from the core is newer (kept apart from app.ts so tests can load it
// without a transport).
import type { AppState } from "../bridge/generated/AppState";

/** Whether `next` may replace `current`: a reply and a pushed state can arrive in either
 * order, and the newer (by revision) wins. */
export function newer(current: AppState | null, next: AppState): boolean {
  return !current || next.revision >= current.revision;
}
