// Application state mirrored from the Rust core. Rust is the source of truth: every
// mutation goes through `rpc`, which returns the complete new state.
import { create } from "zustand";
import { errorMessage, transport } from "../bridge";
import type { AppState } from "../bridge/generated/AppState";
import type { Change } from "../bridge/generated/Change";
import type { EnvSnapshot } from "../bridge/generated/EnvSnapshot";
import type { Request } from "../bridge/generated/Request";
import { toast } from "./toasts";

interface Store {
  state: AppState | null;
  env: EnvSnapshot | null;
  /** Label of a long-running action, shown as a progress overlay. */
  busy: string | null;
  setEnv(env: EnvSnapshot): void;
  rpc(request: Request, busy?: string): Promise<boolean>;
  commit(label: string, changes: Change[]): Promise<boolean>;
}

export const useApp = create<Store>()((set, get) => ({
  state: null,
  env: null,
  busy: null,
  setEnv: (env) => set({ env }),
  async rpc(request, busy) {
    if (busy) set({ busy });
    try {
      const state = await transport.rpc(request);
      set({ state });
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
}));

export const useProject = () => useApp((s) => s.state?.project ?? null);
