// Report themes. With none set, report pages follow the Omarchy desktop theme live.
// A theme is Power BI theme JSON (imported files keep everything in them); the page's
// design tokens are derived from its background, foreground, accent and data colours,
// the same way the app derives its own from the desktop theme.
import type { Desktop } from "../bridge/generated/Desktop";
import type { ReportTheme } from "../bridge/generated/ReportTheme";
import { deriveTokens, type Tokens } from "../design/theme";

/** Built-in themes. Classic is Power BI's default palette; Daylight and Nightfall
 * are colour-vision-checked palettes (validated for lightness, chroma and
 * colour-blind separation). */
export const BUILT_IN: ReportTheme[] = [
  {
    name: "Classic",
    json: {
      name: "Classic",
      dataColors: ["#118DFF", "#12239E", "#E66C37", "#6B007B", "#E044A7", "#744EC2", "#D9B300", "#D64550"],
      background: "#FFFFFF",
      foreground: "#252423",
      tableAccent: "#118DFF",
    },
  },
  {
    name: "Daylight",
    json: {
      name: "Daylight",
      dataColors: ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"],
      background: "#fcfcfb",
      foreground: "#0b0b0b",
      tableAccent: "#2a78d6",
    },
  },
  {
    name: "Nightfall",
    json: {
      name: "Nightfall",
      dataColors: ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"],
      background: "#1a1a19",
      foreground: "#f0efec",
      tableAccent: "#3987e5",
    },
  },
];

const str = (v: unknown) => (typeof v === "string" && /^#[0-9a-f]{3,8}$/i.test(v) ? v : undefined);

function luminance(hex: string): number {
  const h = hex.replace("#", "");
  const full = h.length === 3 ? h.replace(/./g, (c) => c + c) : h.slice(0, 6);
  const c = (i: number) => parseInt(full.slice(i, i + 2), 16) / 255;
  return 0.2126 * c(0) + 0.7152 * c(2) + 0.0722 * c(4);
}

/** A theme's data colours (as written in it). */
export function dataColors(theme: ReportTheme): string[] {
  const c = (theme.json as { dataColors?: unknown }).dataColors;
  return Array.isArray(c) ? c.map(str).filter((x): x is string => !!x) : [];
}

/** The page's design tokens for a report theme, or null to follow the desktop. */
export function themeTokens(theme: ReportTheme | null | undefined, desktop: Desktop): Tokens | null {
  if (!theme) return null;
  const j = theme.json as Record<string, unknown>;
  const colors = dataColors(theme);
  const background = str(j.background) ?? "#ffffff";
  const foreground = str(j.foreground) ?? (luminance(background) > 0.5 ? "#252423" : "#f0efec");
  const accent = str(j.tableAccent) ?? colors[0] ?? "#118DFF";
  const mode = luminance(background) > 0.5 ? "light" : "dark";
  const tokens = deriveTokens({ name: theme.name, mode, colors: { background, foreground, accent }, fromOmarchy: false }, desktop);
  // The data colours as the theme gives them, in order.
  for (const k of Object.keys(tokens)) if (k.startsWith("--chart-")) delete tokens[k];
  // The palette ends at `--chart-count`, so none of the desktop's colours show
  // through a shorter list.
  const list = colors.length ? colors.slice(0, 10) : ["#118DFF"];
  list.forEach((c, i) => (tokens[`--chart-${i + 1}`] = c));
  tokens["--chart-count"] = String(list.length);
  tokens["--mode"] = mode;
  return tokens;
}

/** Inline style for a themed page: its tokens as CSS custom properties. */
export function themeStyle(tokens: Tokens | null): Record<string, string> {
  if (!tokens) return {};
  const style: Record<string, string> = { colorScheme: tokens["--mode"] === "light" ? "light" : "dark" };
  for (const [k, v] of Object.entries(tokens)) style[k] = v;
  return style;
}
