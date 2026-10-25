// Formats values with Power BI/Excel-style format strings ("#,0.00", "\$#,0;(\$#,0)",
// "0.0%", "#,0,K", "0.00E+00", "yyyy-mm-dd", "dddd d MMMM yyyy HH:mm", "Currency"…)
// using Intl, so separators and digits follow the user's locale.
import type { DataType } from "../bridge/generated/DataType";

interface NumberSection {
  prefix: string;
  suffix: string;
  minInt: number;
  /** The integer part is all `#`: a zero integer part is left out (".5"). */
  optionalInt: boolean;
  minFrac: number;
  maxFrac: number;
  grouping: boolean;
  /** Divide by 1000 this many times (trailing commas, "#,0,"). */
  scale: number;
  percent: boolean;
  /** Scientific notation: digits in the exponent and whether "+" is shown. */
  exponent: { digits: number; plus: boolean } | null;
  /** "@" in a text section. */
  text: boolean;
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

/** Removes Excel colour/condition codes ("[Red]") and keeps locale-tagged currency
 * symbols ("[$€-407]" becomes "€"). */
function stripBrackets(s: string): string {
  // Quoted text and escapes pass through untouched.
  return s.replace(/"[^"]*"|\\.|\[\$([^\]-]*)(?:-[^\]]*)?\]|\[[^\]]*\]/g, (m, sym: string | undefined) =>
    m.startsWith('"') || m.startsWith("\\") ? m : sym !== undefined ? `"${sym}"` : "",
  );
}

function parseNumberSection(raw: string): NumberSection {
  const s = stripBrackets(raw);
  let prefix = "";
  let suffix = "";
  let body = "";
  let seenBody = false;
  let percent = false;
  let exponent: NumberSection["exponent"] = null;
  let text = false;
  // Once text follows the digits, everything after is text too ("0 kr.").
  let ended = false;
  const literal = (x: string) => {
    if (seenBody) {
      suffix += x;
      ended = true;
    } else prefix += x;
  };
  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;
    if (ch === "\\" && i + 1 < s.length) literal(s[++i]!);
    else if (ch === '"') {
      const end = s.indexOf('"', i + 1);
      literal(s.slice(i + 1, end < 0 ? s.length : end));
      i = end < 0 ? s.length : end;
    } else if (ch === "_" && i + 1 < s.length) {
      // "_)" pads with the width of ")"; a space is close enough.
      i++;
      literal(" ");
    } else if (ch === "*" && i + 1 < s.length) {
      i++; // Repeat-to-fill has no meaning in a cell that sizes itself.
    } else if (ch === "@") {
      text = true;
      seenBody = true;
    } else if ((ch === "E" || ch === "e") && seenBody && /[+-]/.test(s[i + 1] ?? "") && /[0#]/.test(s[i + 2] ?? "")) {
      const plus = s[i + 1] === "+";
      let j = i + 2;
      while (j < s.length && /[0#]/.test(s[j]!)) j++;
      exponent = { digits: s.slice(i + 2, j).replace(/#/g, "").length || 1, plus };
      i = j - 1;
    } else if (!ended && "#0?,.".includes(ch) && !(ch === "," && !seenBody) && !(ch === "." && !seenBody && !/[#0?]/.test(s[i + 1] ?? ""))) {
      body += ch === "?" ? "#" : ch;
      seenBody = true;
    } else if (ch === "%") {
      percent = true;
      literal("%");
    } else literal(ch);
  }
  // Trailing commas scale by thousands.
  let scale = 0;
  while (body.endsWith(",")) {
    scale++;
    body = body.slice(0, -1);
  }
  const dot = body.indexOf(".");
  const intPart = dot < 0 ? body : body.slice(0, dot);
  const frac = dot < 0 ? "" : body.slice(dot + 1).replace(/,/g, "");
  const intDigits = intPart.replace(/,/g, "");
  return {
    prefix,
    suffix,
    minInt: Math.min(21, intDigits.replace(/#/g, "").length),
    optionalInt: intDigits.length > 0 && !intDigits.includes("0"),
    minFrac: frac.replace(/#/g, "").length,
    maxFrac: frac.length,
    grouping: intPart.includes(","),
    scale,
    percent,
    exponent,
    text,
  };
}

const numberFormats = new Map<string, Intl.NumberFormat>();
function nf(locale: string, minInt: number, minFrac: number, maxFrac: number, grouping: boolean): Intl.NumberFormat {
  const key = `${locale}|${minInt}|${minFrac}|${maxFrac}|${grouping}`;
  let f = numberFormats.get(key);
  if (!f) {
    f = new Intl.NumberFormat(locale, {
      minimumIntegerDigits: Math.max(1, minInt),
      minimumFractionDigits: Math.min(minFrac, 20),
      maximumFractionDigits: Math.min(Math.max(minFrac, maxFrac), 20),
      useGrouping: grouping,
    });
    numberFormats.set(key, f);
  }
  return f;
}

function formatSection(v: number, p: NumberSection, locale: string): string {
  // A section of only text ("zero") shows just the text.
  if (p.maxFrac === 0 && p.minInt === 0 && !p.optionalInt && !p.exponent) return `${p.prefix}${p.suffix}`;
  if (p.percent) v *= 100;
  if (p.scale) v /= 1000 ** p.scale;
  let digits: string;
  if (p.exponent) {
    let exp = v === 0 ? 0 : Math.floor(Math.log10(Math.abs(v)));
    let mantissa = v / 10 ** exp;
    // Rounding can carry the mantissa to 10 ("9.99E+00" at two decimals becomes "1.00E+01").
    if (Number(Math.abs(mantissa).toFixed(p.maxFrac)) >= 10) {
      exp++;
      mantissa /= 10;
    }
    const sign = exp < 0 ? "-" : p.exponent.plus ? "+" : "";
    digits = `${nf(locale, p.minInt, p.minFrac, p.maxFrac, false).format(mantissa)}E${sign}${String(Math.abs(exp)).padStart(p.exponent.digits, "0")}`;
  } else {
    digits = nf(locale, p.minInt, p.minFrac, p.maxFrac, p.grouping).format(v);
    if (p.optionalInt && Math.abs(v) < 1) digits = digits.replace(/^(-?)0(?=\D)/, "$1").replace(/^(-?)0$/, "$1");
  }
  return `${p.prefix}${digits}${p.suffix}`;
}

const NAMED_NUMBER: Record<string, string> = {
  "general number": "",
  general: "",
  fixed: "0.00",
  standard: "#,0.00",
  percent: "0.00%",
  scientific: "0.00E+00",
};

/** Formats a number. An empty or "General" format shows the value as is. */
export function formatNumberWith(value: number, format: string, locale: string): string {
  if (!Number.isFinite(value)) return value > 0 ? "∞" : value < 0 ? "-∞" : "NaN";
  const named = format.trim().toLowerCase();
  if (named === "currency") return new Intl.NumberFormat(locale, { style: "currency", currency: currencyFor(locale) }).format(value);
  if (named === "yes/no") return value ? "Yes" : "No";
  if (named === "true/false") return value ? "True" : "False";
  if (named === "on/off") return value ? "On" : "Off";
  if (named in NAMED_NUMBER) format = NAMED_NUMBER[named]!;
  if (!format) return new Intl.NumberFormat(locale, { maximumFractionDigits: 10 }).format(value);

  const secs = sections(format);
  const positive = parseNumberSection(secs[0]!);
  if (value < 0 && secs.length > 1 && secs[1]) {
    const p = parseNumberSection(secs[1]);
    // A negative value that rounds to zero shows as zero, without a sign.
    if (Number(Math.abs(value).toFixed(p.maxFrac)) === 0 && !p.exponent) return formatSection(0, secs.length > 2 && secs[2] ? parseNumberSection(secs[2]) : positive, locale);
    return formatSection(Math.abs(value), p, locale);
  }
  if (value === 0 && secs.length > 2 && secs[2]) return formatSection(0, parseNumberSection(secs[2]), locale);
  if (value < 0) {
    if (Number(Math.abs(value).toFixed(positive.maxFrac)) === 0 && !positive.exponent && !positive.scale) return formatSection(0, positive, locale);
    return `-${formatSection(Math.abs(value), positive, locale)}`;
  }
  return formatSection(value, positive, locale);
}

/** Applies the text section of a format ("@", or the fourth section) to a string. */
export function formatTextWith(value: string, format: string): string {
  const secs = sections(format);
  const sec = secs.length >= 4 ? secs[3]! : secs.length === 1 && secs[0]!.includes("@") ? secs[0]! : null;
  if (!sec) return value;
  const p = parseNumberSection(sec);
  return p.text ? `${p.prefix}${value}${p.suffix}` : `${p.prefix}${p.suffix}`;
}

function currencyFor(locale: string): string {
  const region = new Intl.Locale(locale).maximize().region ?? "US";
  const byRegion: Record<string, string> = {
    US: "USD", GB: "GBP", DK: "DKK", SE: "SEK", NO: "NOK", CH: "CHF", JP: "JPY", CN: "CNY", IN: "INR", AU: "AUD",
    CA: "CAD", NZ: "NZD", BR: "BRL", MX: "MXN", SA: "SAR", AE: "AED", EG: "EGP", PL: "PLN", CZ: "CZK", KR: "KRW",
    TR: "TRY", ZA: "ZAR", IL: "ILS", RU: "RUB", HU: "HUF",
  };
  const euro = ["DE", "FR", "ES", "IT", "NL", "BE", "AT", "PT", "FI", "IE", "GR", "SK", "SI", "LT", "LV", "EE", "LU", "MT", "CY", "HR"];
  return byRegion[region] ?? (euro.includes(region) ? "EUR" : "USD");
}

// ---- Dates and times ----

const NAMED_DATE: Record<string, Intl.DateTimeFormatOptions> = {
  "general date": { dateStyle: "short", timeStyle: "medium" },
  "long date": { dateStyle: "full" },
  "medium date": { dateStyle: "medium" },
  "short date": { dateStyle: "short" },
  "long time": { timeStyle: "medium" },
  "medium time": { hour: "2-digit", minute: "2-digit", hour12: true },
  "short time": { hour: "2-digit", minute: "2-digit", hour12: false },
};

const DATE_TOKENS = /yyyy|yyy|yy|y|MMMM|MMM|MM|M|mmmm|mmm|mm|m|dddd|ddd|dd|d|HH|H|hh|h|nn|n|ss|s|fff|ff|f|tt|t|AM\/PM|am\/pm|A\/P|a\/p|q|"[^"]*"|\\./g;

export function isDateFormat(format: string): boolean {
  const f = format.trim().toLowerCase();
  if (f in NAMED_DATE) return true;
  const bare = format.replace(/\\./g, "").replace(/"[^"]*"/g, "");
  return /[ydhsHMmn]/.test(bare) && !/[#0@]/.test(bare);
}

/** Formats a date with VB-style (Excel: "mm" is month unless next to hours or seconds)
 * or .NET-style ("MM" month, "mm" minutes) tokens. Dates are wall-clock (UTC fields). */
export function formatDate(value: Date, format: string, locale: string, timeZone = "UTC"): string {
  const named = NAMED_DATE[format.trim().toLowerCase()];
  if (named) return value.toLocaleString(locale, { ...named, timeZone });
  const z = timeZone === "UTC";
  const Y = z ? value.getUTCFullYear() : value.getFullYear();
  const Mo = z ? value.getUTCMonth() : value.getMonth();
  const D = z ? value.getUTCDate() : value.getDate();
  const H = z ? value.getUTCHours() : value.getHours();
  const Mi = z ? value.getUTCMinutes() : value.getMinutes();
  const S = z ? value.getUTCSeconds() : value.getSeconds();
  const Ms = z ? value.getUTCMilliseconds() : value.getMilliseconds();
  const pad = (n: number, w = 2) => String(n).padStart(w, "0");
  const dotnet = /M/.test(format.replace(/"[^"]*"|\\./g, ""));
  const tokens = [...format.matchAll(DATE_TOKENS)];
  const twelve = tokens.some((m) => /^(AM\/PM|am\/pm|A\/P|a\/p|tt|t)$/.test(m[0]));
  const h12 = H % 12 === 0 ? 12 : H % 12;
  const kind = (i: number) => {
    const tk = tokens[i]?.[0] ?? "";
    return /^[hH]/.test(tk) ? "h" : /^s/.test(tk) ? "s" : "";
  };
  let out = "";
  let last = 0;
  tokens.forEach((m, i) => {
    out += format.slice(last, m.index);
    last = m.index! + m[0].length;
    const tok = m[0];
    if (tok.startsWith('"')) return void (out += tok.slice(1, -1));
    if (tok.startsWith("\\")) return void (out += tok[1]);
    // Minutes: "nn" always; "mm"/"m" in .NET style; in VB style next to hours/seconds.
    const isMinute = (tok === "mm" || tok === "m") && (dotnet || kind(i - 1) === "h" || kind(i + 1) === "s");
    if (isMinute) return void (out += tok === "mm" ? pad(Mi) : Mi);
    switch (tok) {
      case "yyyy": case "yyy": out += pad(Y, 4); break;
      case "yy": out += pad(Y % 100); break;
      case "y": out += Y % 100; break;
      case "MMMM": case "mmmm": out += value.toLocaleString(locale, { month: "long", timeZone }); break;
      case "MMM": case "mmm": out += value.toLocaleString(locale, { month: "short", timeZone }); break;
      case "MM": case "mm": out += pad(Mo + 1); break;
      case "M": case "m": out += Mo + 1; break;
      case "dddd": out += value.toLocaleString(locale, { weekday: "long", timeZone }); break;
      case "ddd": out += value.toLocaleString(locale, { weekday: "short", timeZone }); break;
      case "dd": out += pad(D); break;
      case "d": out += D; break;
      case "HH": out += pad(H); break;
      case "H": out += H; break;
      case "hh": out += pad(twelve ? h12 : H); break;
      case "h": out += twelve ? h12 : H; break;
      case "nn": out += pad(Mi); break;
      case "n": out += Mi; break;
      case "ss": out += pad(S); break;
      case "s": out += S; break;
      case "fff": out += pad(Ms, 3); break;
      case "ff": out += pad(Math.floor(Ms / 10)); break;
      case "f": out += Math.floor(Ms / 100); break;
      case "tt": case "AM/PM": out += H < 12 ? "AM" : "PM"; break;
      case "am/pm": out += H < 12 ? "am" : "pm"; break;
      case "t": case "A/P": out += H < 12 ? "A" : "P"; break;
      case "a/p": out += H < 12 ? "a" : "p"; break;
      case "q": out += Math.floor(Mo / 3) + 1; break;
      default: out += tok;
    }
  });
  return out + format.slice(last);
}

/** Parses the forms dates take on their way to the UI: Arrow numbers (ms since the
 * epoch), Date objects, and ISO text from JSON ("2025-01-31", "2025-01-31 10:00:00"). */
export function toDate(value: unknown): Date | null {
  if (value instanceof Date) return value;
  if (typeof value === "number") return new Date(value);
  if (typeof value === "bigint") return new Date(Number(value));
  if (typeof value === "string") {
    let s = value.trim();
    if (/^\d{4}-\d{2}-\d{2} \d/.test(s)) s = s.replace(" ", "T");
    if (/^\d{4}-\d{2}-\d{2}T[\d:.]+$/.test(s)) s += "Z";
    if (/^\d{1,2}:\d{2}/.test(s)) s = `1970-01-01T${s}Z`;
    const d = new Date(s);
    return Number.isNaN(d.getTime()) ? null : d;
  }
  return null;
}

/** Formats a raw cell value for display. */
export function formatValue(value: unknown, type: DataType, format: string | null | undefined, locale: string): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "bigint") value = type === "Time" ? Number(value) / 1000 : Number(value);
  if (value instanceof Date || type === "Date" || type === "DateTime" || type === "DateTimeTz" || type === "Time") {
    const d = toDate(value);
    if (!d) return String(value);
    const zone = type === "DateTimeTz" ? Intl.DateTimeFormat().resolvedOptions().timeZone : "UTC";
    if (format && isDateFormat(format)) return formatDate(d, format, locale, zone);
    if (type === "Date") return d.toLocaleDateString(locale, { timeZone: "UTC" });
    if (type === "Time") return d.toLocaleTimeString(locale, { timeZone: "UTC" });
    return d.toLocaleString(locale, { timeZone: zone });
  }
  if (typeof value === "number") {
    if (format) return formatNumberWith(value, format, locale);
    return type === "Int64" ? nf(locale, 1, 0, 0, false).format(value) : new Intl.NumberFormat(locale, { maximumFractionDigits: 6 }).format(value);
  }
  if (typeof value === "boolean") {
    const f = format?.trim().toLowerCase();
    if (f === "yes/no") return value ? "Yes" : "No";
    if (f === "on/off") return value ? "On" : "Off";
    return value ? "True" : "False";
  }
  const s = String(value);
  return format ? formatTextWith(s, format) : s;
}
