import { create } from "zustand";

export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  title: string;
  detail?: string;
}

interface ToastStore {
  toasts: Toast[];
  dismiss(id: number): void;
}

let next = 1;

export const useToasts = create<ToastStore>((set) => ({
  toasts: [],
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/** Shows a toast. Errors stay until dismissed; others fade after a few seconds. */
export function toast(t: Omit<Toast, "id">) {
  const id = next++;
  useToasts.setState((s) => ({ toasts: [...s.toasts.slice(-3), { ...t, id }] }));
  if (t.kind !== "error") setTimeout(() => useToasts.getState().dismiss(id), 3500);
}
