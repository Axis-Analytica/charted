// View as: previewing the report as the members of row-level security roles see it.
// View state (not saved, not undoable): the roles' filters go into every query the
// report makes, and a banner says so until the user stops viewing.
import { create } from "zustand";

interface ViewAsStore {
  /** The project the roles belong to (viewing ends with the project). */
  project: string | null;
  /** Ids of the roles viewed as; empty when not viewing. */
  roles: string[];
  set(project: string | null, roles: string[]): void;
  stop(): void;
}

const NONE: string[] = [];

export const useViewAs = create<ViewAsStore>()((set) => ({
  project: null,
  roles: NONE,
  set: (project, roles) => set({ project, roles: roles.length ? roles : NONE }),
  stop: () => set({ roles: NONE }),
}));

/** The roles viewed as in `project` (a stable empty list otherwise). */
export function viewedRoles(s: { project: string | null; roles: string[] }, project: string | undefined): string[] {
  return s.project === project ? s.roles : NONE;
}
