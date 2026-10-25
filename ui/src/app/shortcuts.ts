// The shortcut registry. Every binding is declared here once; the keyboard handler and
// the `?` overlay both read it, so the overlay can never drift from what is bound.
import { useEffect } from "react";
import { create } from "zustand";

export interface Shortcut {
  id: string;
  /** e.g. "Ctrl+Z", "Ctrl+Shift+Z", "?", "Q", "F6". */
  keys: string;
  /** English label; translated when shown. */
  label: string;
  group: "General" | "Edit" | "File" | "View" | "Navigate";
  run(): void;
  /** When false the shortcut is listed but inactive. */
  enabled?: () => boolean;
  /** Fire even while typing in a text field. */
  global?: boolean;
}

interface Registry {
  shortcuts: Map<string, Shortcut>;
}

export const useShortcuts = create<Registry>(() => ({ shortcuts: new Map() }));

/** Registers shortcuts for the lifetime of the calling component. */
export function useShortcut(...items: Shortcut[]) {
  useEffect(() => {
    const map = new Map(useShortcuts.getState().shortcuts);
    for (const s of items) map.set(s.id, s);
    useShortcuts.setState({ shortcuts: map });
    return () => {
      const m = new Map(useShortcuts.getState().shortcuts);
      for (const s of items) if (m.get(s.id) === s) m.delete(s.id);
      useShortcuts.setState({ shortcuts: m });
    };
  });
}

function eventKeys(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  const printable = e.key.length === 1;
  // Shift is part of the character for printable symbols like "?".
  if (e.shiftKey && (!printable || /[a-z]/i.test(e.key))) parts.push("Shift");
  parts.push(printable ? e.key.toUpperCase() : e.key);
  return parts.join("+");
}

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  return el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName) || !!el.closest(".monaco-editor");
}

export function installShortcutHandler(): () => void {
  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented) return;
    const keys = eventKeys(e);
    const typing = isTyping(e.target);
    for (const s of useShortcuts.getState().shortcuts.values()) {
      if (s.keys.toUpperCase() !== keys.toUpperCase()) continue;
      if (typing && !s.global) continue;
      if (s.enabled && !s.enabled()) continue;
      e.preventDefault();
      s.run();
      return;
    }
  };
  window.addEventListener("keydown", onKey);
  return () => window.removeEventListener("keydown", onKey);
}
