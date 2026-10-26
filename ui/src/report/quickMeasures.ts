// Quick measures: common calculations written as DAX from a few picks. Every template
// here is checked on the sample by crates/core/tests/quickmeasures.rs.
import type { Model } from "../bridge/generated/Model";
import { msg } from "../i18n";

export type Calc =
  | "sum"
  | "average"
  | "countRows"
  | "distinct"
  | "weightedAverage"
  | "averagePer"
  | "maxPer"
  | "minPer"
  | "shareOfTotal"
  | "filtered"
  | "ytd"
  | "qtd"
  | "mtd"
  | "yoyChange"
  | "yoyPercent"
  | "ratio"
  | "difference"
  | "percentDifference";

/** A calculation and what it asks for. */
export interface CalcDef {
  calc: Calc;
  label: string;
  group: string;
  /** A value: a measure, or a numeric column (summed). */
  value?: boolean;
  second?: boolean;
  /** Any column (counted). */
  column?: boolean;
  table?: boolean;
  date?: boolean;
  category?: boolean;
  /** A text the category is compared with. */
  text?: boolean;
  /** A numeric column and its weight, from one table. */
  weighted?: boolean;
}

export const CALCS: CalcDef[] = [
  { calc: "sum", label: msg("Total"), group: msg("Aggregate"), value: true },
  { calc: "average", label: msg("Average"), group: msg("Aggregate"), value: true },
  { calc: "countRows", label: msg("Count of rows"), group: msg("Aggregate"), table: true },
  { calc: "distinct", label: msg("Count (distinct)"), group: msg("Aggregate"), column: true },
  { calc: "weightedAverage", label: msg("Weighted average"), group: msg("Aggregate"), weighted: true },
  { calc: "averagePer", label: msg("Average per category"), group: msg("Per category"), value: true, category: true },
  { calc: "maxPer", label: msg("Largest per category"), group: msg("Per category"), value: true, category: true },
  { calc: "minPer", label: msg("Smallest per category"), group: msg("Per category"), value: true, category: true },
  { calc: "shareOfTotal", label: msg("Percentage of total"), group: msg("Per category"), value: true, category: true },
  { calc: "filtered", label: msg("Filtered value"), group: msg("Filters"), value: true, category: true, text: true },
  { calc: "ytd", label: msg("Year-to-date total"), group: msg("Time intelligence"), value: true, date: true },
  { calc: "qtd", label: msg("Quarter-to-date total"), group: msg("Time intelligence"), value: true, date: true },
  { calc: "mtd", label: msg("Month-to-date total"), group: msg("Time intelligence"), value: true, date: true },
  { calc: "yoyChange", label: msg("Year-over-year change"), group: msg("Time intelligence"), value: true, date: true },
  { calc: "yoyPercent", label: msg("Year-over-year change %"), group: msg("Time intelligence"), value: true, date: true },
  { calc: "ratio", label: msg("Ratio (divide)"), group: msg("Mathematical operations"), value: true, second: true },
  { calc: "difference", label: msg("Difference"), group: msg("Mathematical operations"), value: true, second: true },
  { calc: "percentDifference", label: msg("Percentage difference"), group: msg("Mathematical operations"), value: true, second: true },
];

export interface FieldOption {
  /** The DAX reference: `[Measure]` or `'Table'[Column]`. */
  ref: string;
  label: string;
  short: string;
  kind: "measure" | "column";
  numeric: boolean;
  date: boolean;
  table: string;
}

export const quoteTable = (s: string) => (/^[A-Za-z_][A-Za-z0-9_]*$/.test(s) ? s : `'${s.replace(/'/g, "''")}'`);

/** Every measure and column as a DAX reference; date columns of a marked date table first. */
export function fieldOptions(model: Model): FieldOption[] {
  const out: FieldOption[] = [];
  for (const tb of model.tables) {
    for (const m of tb.measures) out.push({ ref: `[${m.name}]`, label: m.name, short: m.name, kind: "measure", numeric: true, date: false, table: tb.name });
    for (const c of tb.columns) {
      const numeric = c.dataType === "Int64" || c.dataType === "Double" || c.dataType === "Decimal";
      const date = c.dataType === "Date" || c.dataType === "DateTime" || c.dataType === "DateTimeTz";
      out.push({ ref: `${quoteTable(tb.name)}[${c.name}]`, label: `${tb.name}[${c.name}]`, short: c.name, kind: "column", numeric, date, table: tb.name });
    }
  }
  const marked = new Set(model.tables.filter((t) => t.isDateTable).map((t) => t.name));
  return out.sort((a, b) => Number(b.date && marked.has(b.table)) - Number(a.date && marked.has(a.table)));
}

export interface Picks {
  value: string;
  second: string;
  column: string;
  table: string;
  date: string;
  category: string;
  text: string;
  weight: string;
}

/** The measure a calculation makes: its name, DAX and format string. */
export function quickMeasure(calc: Calc, p: Picks, fields: FieldOption[]): { name: string; expression: string; format: string | null } {
  const f = (r: string) => fields.find((x) => x.ref === r);
  const label = (r: string) => f(r)?.short ?? r;
  // A column is summed; a measure is used as it is.
  const v = (r: string) => (f(r)?.kind === "column" ? `SUM ( ${r} )` : r);
  const V = v(p.value);
  const prior = `CALCULATE ( ${V}, SAMEPERIODLASTYEAR ( ${p.date} ) )`;
  const out = (name: string, expression: string, format: string | null = null) => ({ name: name.replace(/[[\]]/g, ""), expression, format });
  const pct = "0.0%";
  switch (calc) {
    case "sum":
      return out(`Total ${label(p.value)}`, V);
    case "average":
      return out(`Average ${label(p.value)}`, f(p.value)?.kind === "column" ? `AVERAGE ( ${p.value} )` : V);
    case "countRows":
      return out(`${p.table} rows`, `COUNTROWS ( ${quoteTable(p.table)} )`);
    case "distinct":
      return out(`Distinct ${label(p.column)}`, `DISTINCTCOUNT ( ${p.column} )`);
    case "weightedAverage":
      return out(`${label(p.value)} weighted by ${label(p.weight)}`, `DIVIDE ( SUMX ( ${quoteTable(f(p.value)?.table ?? p.table)}, ${p.value} * ${p.weight} ), SUM ( ${p.weight} ) )`);
    case "averagePer":
      return out(`Average ${label(p.value)} per ${label(p.category)}`, `AVERAGEX ( VALUES ( ${p.category} ), ${V} )`);
    case "maxPer":
      return out(`Largest ${label(p.value)} per ${label(p.category)}`, `MAXX ( VALUES ( ${p.category} ), ${V} )`);
    case "minPer":
      return out(`Smallest ${label(p.value)} per ${label(p.category)}`, `MINX ( VALUES ( ${p.category} ), ${V} )`);
    case "shareOfTotal":
      return out(`${label(p.value)} % of total`, `DIVIDE ( ${V}, CALCULATE ( ${V}, ALLSELECTED ( ${p.category} ) ) )`, pct);
    case "filtered":
      return out(`${label(p.value)} for ${p.text}`, `CALCULATE ( ${V}, ${p.category} = "${p.text.replace(/"/g, '""')}" )`);
    case "ytd":
      return out(`${label(p.value)} YTD`, `TOTALYTD ( ${V}, ${p.date} )`);
    case "qtd":
      return out(`${label(p.value)} QTD`, `TOTALQTD ( ${V}, ${p.date} )`);
    case "mtd":
      return out(`${label(p.value)} MTD`, `TOTALMTD ( ${V}, ${p.date} )`);
    case "yoyChange":
      return out(`${label(p.value)} YoY`, `${V} - ${prior}`);
    case "yoyPercent":
      return out(`${label(p.value)} YoY %`, `VAR prior = ${prior} RETURN DIVIDE ( ${V} - prior, prior )`, pct);
    case "ratio":
      return out(`${label(p.value)} per ${label(p.second)}`, `DIVIDE ( ${V}, ${v(p.second)} )`);
    case "difference":
      return out(`${label(p.value)} - ${label(p.second)}`, `${V} - ${v(p.second)}`);
    case "percentDifference":
      return out(`${label(p.value)} vs ${label(p.second)} %`, `DIVIDE ( ${V} - ${v(p.second)}, ${v(p.second)} )`, pct);
  }
}
