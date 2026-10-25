// Translation runtime. Catalogs are English-keyed JSON (Omarchy's convention): a plain
// string, or plural forms keyed by Intl.PluralRules categories. Placeholders are %1, %2…
// and %n for the count. Lookups fall back from "pt-BR" to "pt" to English per string.
import { create } from "zustand";

type Entry = string | Partial<Record<Intl.LDMLPluralRule, string>>;
type Catalog = Record<string, Entry>;

const catalogs: Record<string, Catalog> = Object.fromEntries(
  Object.entries(import.meta.glob<Catalog>("../../../i18n/*.json", { eager: true, import: "default" })).map(([path, cat]) => [
    path.split("/").pop()!.replace(/\.json$/, ""),
    cat,
  ]),
);

interface LanguageState {
  code: string;
  locale: string;
  rtl: boolean;
  set(code: string, locale: string, rtl: boolean): void;
}

export const useLanguage = create<LanguageState>((set) => ({
  code: "en",
  locale: "en-US",
  rtl: false,
  set: (code, locale, rtl) => {
    document.documentElement.lang = code;
    document.documentElement.dir = rtl ? "rtl" : "ltr";
    set({ code, locale, rtl });
  },
}));

function chain(code: string): Catalog[] {
  const out: Catalog[] = [];
  const exact = catalogs[code];
  if (exact) out.push(exact);
  const base = code.split("-")[0]!;
  if (base !== code && catalogs[base]) out.push(catalogs[base]!);
  return out;
}

function fill(text: string, args: (string | number)[], n?: number, locale?: string): string {
  return text.replace(/%(\d+|n)/g, (m, k: string) => {
    if (k === "n") return n === undefined ? m : new Intl.NumberFormat(locale).format(n);
    const v = args[Number(k) - 1];
    return v === undefined ? m : typeof v === "number" ? new Intl.NumberFormat(locale).format(v) : v;
  });
}

/** Translates `text`, filling %1, %2… with `args`. */
export function t(text: string, ...args: (string | number)[]): string {
  const { code, locale } = useLanguage.getState();
  for (const cat of chain(code)) {
    const e = cat[text];
    if (typeof e === "string" && e) return fill(e, args, undefined, locale);
  }
  return fill(text, args, undefined, locale);
}

/** Translates a counted string, e.g. tn("%n rows", 3). `text` is the English "other" form; `one` the singular. */
export function tn(text: string, n: number, one?: string, ...args: (string | number)[]): string {
  const { code, locale } = useLanguage.getState();
  const rule = new Intl.PluralRules(locale).select(n);
  for (const cat of chain(code)) {
    const e = cat[text];
    if (e && typeof e === "object") {
      const form = e[rule] ?? e.other;
      if (form) return fill(form, args, n, locale);
    }
  }
  const english = new Intl.PluralRules("en").select(n) === "one" && one ? one : text;
  return fill(english, args, n, locale);
}

/** Re-renders the calling component when the language changes. */
export function useT() {
  useLanguage((s) => s.code);
  return { t, tn };
}

export function formatNumber(n: number, options?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(useLanguage.getState().locale, options).format(n);
}
