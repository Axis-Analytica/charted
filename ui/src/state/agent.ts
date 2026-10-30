// The in-app agent's state, mirrored from the core (crates/agent/src/host.rs): the
// conversation, which agent runs, and whether it may see data. While a request runs the
// canvas follows the pages it changes, until the user goes elsewhere themselves.
import { create } from "zustand";
import { errorMessage, isApiError, transport } from "../bridge";
import type { AgentEvent } from "../bridge/generated/AgentEvent";
import type { AgentRequest } from "../bridge/generated/AgentRequest";
import type { AgentStatus } from "../bridge/generated/AgentStatus";
import type { TurnRecord } from "../bridge/generated/TurnRecord";
import { useReport } from "./report";
import { toast } from "./toasts";

interface AgentStore {
  status: AgentStatus | null;
  /** The consent dialog is open, with the request to send once agreed. */
  consent: { text: string; page: string | null; visuals: string[] } | null;
  /** Visuals the agent just touched, by id, until when (ms) they're outlined. */
  touched: Record<string, number>;
  /** The composer's text, kept while the pane is closed. */
  draft: string;
  set(patch: Partial<Pick<AgentStore, "consent" | "draft">>): void;
  request(r: AgentRequest): Promise<boolean>;
  send(text: string, page: string | null, visuals: string[]): Promise<boolean>;
}

export const useAgent = create<AgentStore>()((set) => ({
  status: null,
  consent: null,
  touched: {},
  draft: "",
  set: (patch) => set(patch),
  async request(r) {
    try {
      set({ status: await transport.agent(r) });
      return true;
    } catch (e) {
      toast({ kind: "error", title: errorMessage(e) });
      return false;
    }
  },
  async send(text, page, visuals) {
    try {
      follow = { moved: false, seen: new Map() };
      set({ status: await transport.agent({ type: "send", text, page, visuals }) });
      return true;
    } catch (e) {
      if (isApiError(e) && e.code === "consent") {
        set({ consent: { text, page, visuals } });
        return false;
      }
      toast({ kind: "error", title: errorMessage(e) });
      return false;
    }
  },
}));

/** Following the agent: whether the user moved to another page during the request, and
 * how many items of each turn have been acted on. */
let follow: { moved: boolean; seen: Map<string, number> } = { moved: false, seen: new Map() };
let navigating = false;

function applyTurn(turn: TurnRecord) {
  const s = useAgent.getState().status;
  if (!s) return;
  const turns = s.turns.some((t) => t.id === turn.id) ? s.turns.map((t) => (t.id === turn.id ? turn : t)) : [...s.turns, turn];
  const seen = follow.seen.get(turn.id) ?? 0;
  follow.seen.set(turn.id, turn.items.length);
  const touched = { ...useAgent.getState().touched };
  const until = Date.now() + 1600;
  for (const item of turn.items.slice(seen)) {
    if (item.kind !== "step" || !item.step.ok) continue;
    for (const v of item.step.visuals) touched[v] = until;
    const page = item.step.page;
    if (page && !follow.moved && turn.status === "running" && useReport.getState().page !== page) {
      navigating = true;
      useReport.getState().set({ page, selected: [] });
      navigating = false;
    }
  }
  useAgent.setState({ status: { ...s, turns, running: turn.status === "running" ? true : s.running }, touched });
  // Drop the outlines once they've faded.
  setTimeout(() => {
    const now = Date.now();
    const left = Object.fromEntries(Object.entries(useAgent.getState().touched).filter(([, t]) => t > now));
    useAgent.setState({ touched: left });
  }, 1700);
}

/** Subscribes to the agent's pushes (once, from App). */
export function initAgent(): () => void {
  const off = transport.onAgent((e: AgentEvent) => {
    if (e.kind === "status") useAgent.setState({ status: e.status });
    else applyTurn(e.turn);
  });
  const offNav = useReport.subscribe((s, prev) => {
    // The view picking its first page isn't the user going somewhere.
    if (prev.page && s.page !== prev.page && !navigating && useAgent.getState().status?.running) follow.moved = true;
  });
  return () => {
    off();
    offNav();
  };
}
