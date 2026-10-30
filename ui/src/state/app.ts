// Application state mirrored from the Rust core. Rust is the source of truth: every
// mutation goes through `rpc`, which returns the complete new state.
import { create } from "zustand";
import { errorMessage, transport } from "../bridge";
import type { AppState } from "../bridge/generated/AppState";
import type { Change } from "../bridge/generated/Change";
import type { EnvSnapshot } from "../bridge/generated/EnvSnapshot";
import type { Model } from "../bridge/generated/Model";
import type { Request } from "../bridge/generated/Request";
import { newer } from "./revision";
import { toast } from "./toasts";

interface Store {
  state: AppState | null;
  env: EnvSnapshot | null;
  /** Label of a long-running action, shown as a progress overlay. */
  busy: string | null;
  setEnv(env: EnvSnapshot): void;
  rpc(request: Request, busy?: string): Promise<boolean>;
  commit(label: string, changes: Change[]): Promise<boolean>;
  /** Builds changes from the latest model once earlier edits have landed, then commits
   * them. Use it when a change depends on current state, so quick successive edits
   * (ticking several fields) never overwrite each other. */
  edit(label: string, build: (model: Model) => Change[] | null | undefined): Promise<boolean>;
}

let queue: Promise<unknown> = Promise.resolve();

/** Takes a state from the core unless a newer one has arrived already. */
export function applyState(state: AppState) {
  if (newer(useApp.getState().state, state)) useApp.setState({ state });
}

export const useApp = create<Store>()((set, get) => ({
  state: null,
  env: null,
  busy: null,
  setEnv: (env) => set({ env }),
  async rpc(request, busy) {
    if (busy) set({ busy });
    try {
      applyState(await transport.rpc(request));
      return true;
    } catch (e) {
      toast({ kind: "error", title: errorMessage(e) });
      return false;
    } finally {
      if (busy) set({ busy: null });
    }
  },
  commit(label, changes) {
    return get().rpc({ type: "commit", label, changes });
  },
  edit(label, build) {
    const run = queue.then(async () => {
      const model = get().state?.project?.model;
      const changes = model ? build(model) : null;
      if (!changes?.length) return false;
      return get().commit(label, changes);
    });
    queue = run.catch(() => {});
    return run;
  },
}));

export const useProject = () => useApp((s) => s.state?.project ?? null);
