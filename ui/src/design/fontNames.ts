// Report font names: the bundled fonts (fonts.ts loads them), Power BI's usual names with
// a bundled stand-in, and the CSS font list for a name. No imports, so tests and the
// theme code can use it without the app.
/** The bundled fonts, by the name reports use, with the name each is loaded under. */
export const BUNDLED: Record<string, string> = {
  Inter: "Inter Variable",
  Roboto: "Roboto Variable",
  "Open Sans": "Open Sans Variable",
  Lato: "Lato",
  Montserrat: "Montserrat Variable",
  Poppins: "Poppins",
  "Source Sans 3": "Source Sans 3 Variable",
  "Noto Sans": "Noto Sans Variable",
  Nunito: "Nunito Variable",
  "IBM Plex Sans": "IBM Plex Sans Variable",
  Oswald: "Oswald Variable",
  Merriweather: "Merriweather Variable",
  Lora: "Lora Variable",
  "Playfair Display": "Playfair Display Variable",
  "Roboto Mono": "Roboto Mono Variable",
};

/** Power BI's common fonts and the bundled font that stands in when one isn't installed. */
export const POWER_BI: Record<string, string> = {
  "Segoe UI": "Open Sans",
  "Segoe UI Semibold": "Open Sans",
  "Segoe UI Light": "Open Sans",
  DIN: "Roboto",
  Arial: "Roboto",
  Helvetica: "Inter",
  Calibri: "Source Sans 3",
  Candara: "Nunito",
  Corbel: "Source Sans 3",
  Verdana: "Noto Sans",
  Tahoma: "Noto Sans",
  "Trebuchet MS": "Source Sans 3",
  Georgia: "Merriweather",
  Cambria: "Lora",
  "Times New Roman": "Lora",
  "Courier New": "Roboto Mono",
  "Consolas": "Roboto Mono",
};

export const quote = (f: string) => (/^[\w-]+$/.test(f) && !/^\d/.test(f) ? f : `"${f.replace(/["\\]/g, "")}"`);

/** A CSS font-family list for a report's font name (which may itself be a list, as in
 * Power BI themes): each name, the font it's loaded under, its stand-in, then the UI's
 * font. Undefined for no name, so the inherited font applies. */
export function fontStack(name: unknown): string | undefined {
  if (typeof name !== "string" || !name.trim()) return undefined;
  const out: string[] = [];
  const add = (f: string | undefined) => f && !out.includes(f) && out.push(f);
  for (const raw of name.replace(/[;{}]/g, "").split(",")) {
    const f = raw.trim().replace(/^['"]|['"]$/g, "");
    if (!f || /^(sans-serif|serif|monospace|system-ui)$/i.test(f)) continue;
    add(f);
    add(BUNDLED[f]);
    const standIn = POWER_BI[f];
    if (standIn) add(BUNDLED[standIn]);
  }
  return out.length ? `${out.map(quote).join(", ")}, var(--font-ui, sans-serif)` : undefined;
}
