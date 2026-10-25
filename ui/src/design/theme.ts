import type { Desktop } from "../bridge/generated/Desktop";
import type { Theme } from "../bridge/generated/Theme";
import { contrast, distance, ensureContrast, mix } from "./color";

export const DEFAULT_ACCENT = "#FFD60A";

export interface Tokens {
  [name: string]: string;
}

/**
 * Derives the app's semantic design tokens from an Omarchy theme. Keys missing from a
 * theme's colors.toml are derived from the ones present, and text colours are nudged
 * until they meet WCAG contrast against the surfaces they sit on.
 */
export function deriveTokens(theme: Theme, desktop: Desktop): Tokens {
  const c = theme.colors;
  const dark = theme.mode !== "light";
  const bg = c.background ?? (dark ? "#121212" : "#fafafa");
  const fg = c.foreground ?? (dark ? "#e8e8e8" : "#1a1a1a");
  const toward = dark ? "#000000" : fg;
  const sunken = c.dark_background ?? mix(bg, toward, dark ? 0.25 : 0.04);
  const raised = c.lighter_background ?? mix(bg, fg, dark ? 0.05 : 0.0);
  const overlay = c.darker_background ?? mix(bg, toward, dark ? 0.4 : 0.08);
  const panel = dark ? raised : mix(bg, "#ffffff", 0.6);
  const accent = c.accent ?? DEFAULT_ACCENT;
  const accentFg = contrast("#000000", accent) >= contrast("#ffffff", accent) ? "#000000" : "#ffffff";
  const border = c.muted && contrast(c.muted, bg) > 1.25 ? c.muted : mix(bg, fg, dark ? 0.16 : 0.14);
  const muted = ensureContrast(c.dark_foreground ?? mix(fg, bg, 0.42), bg, 4.5);
  const faint = ensureContrast(mix(fg, bg, 0.6), bg, 3);

  const tokens: Tokens = {
    "--bg": bg,
    "--bg-sunken": sunken,
    "--bg-raised": raised,
    "--bg-panel": panel,
    "--bg-overlay": overlay,
    "--bg-hover": mix(bg, fg, dark ? 0.08 : 0.06),
    "--bg-active": mix(bg, fg, dark ? 0.13 : 0.1),
    "--fg": fg,
    "--fg-strong": c.bright_foreground ?? c.light_foreground ?? (dark ? mix(fg, "#ffffff", 0.5) : mix(fg, "#000000", 0.5)),
    "--fg-muted": muted,
    "--fg-faint": faint,
    "--border": border,
    "--border-strong": mix(border, fg, 0.25),
    "--selection": c.selection ?? mix(bg, accent, 0.25),
    "--accent": accent,
    "--accent-fg": accentFg,
    "--accent-soft": mix(bg, accent, dark ? 0.16 : 0.14),
    "--accent-text": ensureContrast(accent, bg, 4.5),
    "--danger": ensureContrast(c.red ?? "#e5484d", bg, 3),
    "--warning": ensureContrast(c.yellow ?? "#f5a524", bg, 3),
    "--success": ensureContrast(c.green ?? "#30a46c", bg, 3),
    "--info": ensureContrast(c.blue ?? "#3e8ef7", bg, 3),
    "--danger-soft": mix(bg, c.red ?? "#e5484d", 0.16),
    "--warning-soft": mix(bg, c.yellow ?? "#f5a524", 0.16),
    "--radius": `${desktop.rounding}px`,
    "--radius-sm": `${Math.min(4, desktop.rounding)}px`,
    "--shadow": dark ? "0 8px 28px rgb(0 0 0 / 0.45)" : "0 8px 28px rgb(0 0 0 / 0.14)",
  };
  chartPalette(theme, bg).forEach((color, i) => {
    tokens[`--chart-${i + 1}`] = color;
  });
  return tokens;
}

/**
 * A categorical chart palette built from the theme's own colours: the accent first,
 * then the ANSI hues, dropping any too close to the background or to an earlier pick.
 */
export function chartPalette(theme: Theme, bg: string): string[] {
  const c = theme.colors;
  const keys = ["accent", "blue", "green", "magenta", "cyan", "yellow", "red", "orange", "bright_blue", "bright_green", "bright_magenta", "bright_cyan", "bright_yellow", "bright_red"];
  const out: string[] = [];
  for (const k of keys) {
    const v = c[k];
    if (!v) continue;
    const color = ensureContrast(v, bg, 3);
    if (out.every((o) => distance(o, color) > 9)) out.push(color);
    if (out.length === 10) break;
  }
  const fallbacks = ["#4e79a7", "#f28e2b", "#59a14f", "#e15759", "#76b7b2", "#edc948", "#b07aa1", "#ff9da7"];
  for (const f of fallbacks) {
    if (out.length >= 8) break;
    const color = ensureContrast(f, bg, 3);
    if (out.every((o) => distance(o, color) > 9)) out.push(color);
  }
  return out;
}

export function applyTokens(tokens: Tokens, mode: string, animations: boolean) {
  const root = document.documentElement;
  for (const [k, v] of Object.entries(tokens)) root.style.setProperty(k, v);
  root.style.colorScheme = mode === "light" ? "light" : "dark";
  root.dataset.motion = animations && !matchMedia("(prefers-reduced-motion: reduce)").matches ? "on" : "off";
}
