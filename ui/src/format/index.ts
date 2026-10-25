// Formats values with Power BI/Excel-style format strings ("#,0.00", "\$#,0;(\$#,0)",
// "0.0%", "yyyy-mm-dd"…) using Intl, so separators and digits follow the user's locale.
import type { DataType } from "../bridge/generated/DataType";

interface NumberSection {
  prefix: string;
  suffix: string;
  decimals: number;
  grouping: boolean;
  percent: boolean;
}

/** Splits on unescaped, unquoted semicolons. */
function sections(format: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quoted = false;
  for (let i = 0; i < format.length; i++) {
    const ch = format[i]!;
    if (ch === "\\" && i + 1 < format.length) {
      cur += ch + format[++i];
    } else if (ch === '"') {
      quoted = !quoted;
      cur += ch;
    } else if (ch === ";" && !quoted) {
      out.push(cur);
      cur = "";
    } else cur += ch;
  }
  out.push(cur);
  return out;
}

function parseNumberSection(s: string): NumberSection {
  let prefix = "";
  let suffix = "";
  let body = "";
  let seenBody = false;
  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;
    let literal: string | null = null;
    if (ch === "\\" && i + 1 < s.length) literal = s[++i]!;
    else if (ch === '"') {
      const end = s.indexOf('"', i + 1);
      literal = s.slice(i + 1, end < 0 ? s.length : end);
      i = end < 0 ? s.length : end;
    } else if ("#0,.".includes(ch)) {
      body += ch;
      seenBody = true;
      continue;
    } else if (ch === "%") {
      body += ch;
      continue;
    } else literal = ch;
    if (seenBody) suffix += literal;
    else prefix += literal;
  }
  const dot = body.indexOf(".");
  const decimals = dot < 0 ? 0 : body.slice(dot + 1).replace(/[^0#]/g, "").length;
  const intPart = dot < 0 ? body : body.slice(0, dot);
  return { prefix, suffix, decimals, grouping: intPart.includes(","), percent: body.includes("%") };
}

const DATE_TOKENS = /yyyy|yy|mmmm|mmm|mm|m|dddd|ddd|dd|d|hh|h|nn|ss|AM\/PM/gi;

function isDateFormat(f: string): boolean {
  return /[yd]/i.test(f.replace(/\\./g, "").replace(/"[^"]*"/g, "")) && !/[#0]/.test(f);
}

function formatDate(value: Date, format: string, locale: string): string {
  const pad = (n: number, w = 2) => String(n).padStart(w, "0");
  const after = (i: number) => format.slice(i);
  let out = "";
  let last = 0;
  // "mm" means minutes after an hour token, months otherwise.
  let sawHour = false;
  for (const m of format.matchAll(DATE_TOKENS)) {
    out += format.slice(last, m.index).replace(/\\(.)/g, "$1").replace(/"([^"]*)"/g, "$1");
    last = m.index! + m[0].length;
    const tok = m[0];
    const lower = tok.toLowerCase();
    switch (lower) {
      case "yyyy": out += value.getUTCFullYear(); break;
      case "yy": out += pad(value.getUTCFullYear() % 100); break;
      case "mmmm": out += value.toLocaleString(locale, { month: "long", timeZone: "UTC" }); break;
      case "mmm": out += value.toLocaleString(locale, { month: "short", timeZone: "UTC" }); break;
      case "mm": out += sawHour || /^:?s/.test(after(last)) ? pad(value.getUTCMinutes()) : pad(value.getUTCMonth() + 1); break;
      case "m": out += value.getUTCMonth() + 1; break;
      case "dddd": out += value.toLocaleString(locale, { weekday: "long", timeZone: "UTC" }); break;
      case "ddd": out += value.toLocaleString(locale, { weekday: "short", timeZone: "UTC" }); break;
      case "dd": out += pad(value.getUTCDate()); break;
      case "d": out += value.getUTCDate(); break;
      case "hh": sawHour = true; out += pad(value.getUTCHours()); break;
      case "h": sawHour = true; out += value.getUTCHours(); break;
      case "nn": out += pad(value.getUTCMinutes()); break;
      case "ss": out += pad(value.getUTCSeconds()); break;
      default: out += tok;
    }
  }
  return out + format.slice(last).replace(/\\(.)/g, "$1").replace(/"([^"]*)"/g, "$1");
}

const numberFormats = new Map<string, Intl.NumberFormat>();
function nf(locale: string, decimals: number, grouping: boolean): Intl.NumberFormat {
  const key = `${locale}|${decimals}|${grouping}`;
  let f = numberFormats.get(key);
  if (!f) {
    f = new Intl.NumberFormat(locale, { minimumFractionDigits: decimals, maximumFractionDigits: decimals, useGrouping: grouping });
    numberFormats.set(key, f);
  }
  return f;
}

export function formatNumberWith(value: number, format: string, locale: string): string {
  const secs = sections(format);
  let sec = secs[0]!;
  let v = value;
  if (value < 0 && secs.length > 1 && secs[1]) {
    sec = secs[1];
    v = Math.abs(value);
  } else if (value === 0 && secs.length > 2 && secs[2]) {
    sec = secs[2];
  }
  const p = parseNumberSection(sec);
  if (p.percent) v *= 100;
  return `${p.prefix}${nf(locale, p.decimals, p.grouping).format(v)}${p.suffix}`;
}

/** Formats a raw cell value for display. */
export function formatValue(value: unknown, type: DataType, format: string | null | undefined, locale: string): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "bigint") value = Number(value);
  if (value instanceof Date || type === "Date" || type === "DateTime" || type === "DateTimeTz") {
    const d = value instanceof Date ? value : new Date(typeof value === "number" ? value : String(value));
    if (Number.isNaN(d.getTime())) return String(value);
    if (format && isDateFormat(format)) return formatDate(d, format, locale);
    return type === "Date"
      ? d.toLocaleDateString(locale, { timeZone: "UTC" })
      : d.toLocaleString(locale, { timeZone: type === "DateTimeTz" ? undefined : "UTC" });
  }
  if (typeof value === "number") {
    if (format) return formatNumberWith(value, format, locale);
    return type === "Int64" ? nf(locale, 0, false).format(value) : new Intl.NumberFormat(locale, { maximumFractionDigits: 6 }).format(value);
  }
  if (typeof value === "boolean") return value ? "True" : "False";
  return String(value);
}
