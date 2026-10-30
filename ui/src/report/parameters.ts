// What-if parameters (Power BI's "New parameter → Numeric range"): the range checks, and
// the calculated table they make: GENERATESERIES values in a column named after the
// parameter, with a measure of the value picked (SELECTEDVALUE, or the default).
import type { Column } from "../bridge/generated/Column";
import type { Measure } from "../bridge/generated/Measure";
import type { Table } from "../bridge/generated/Table";
import { msg } from "../i18n";
import { quoteTable } from "./quickMeasures";

export interface ParameterSpec {
  name: string;
  decimal: boolean;
  min: number;
  max: number;
  step: number;
  default: number;
}

/** Problems with a parameter's range, or null. */
export function parameterProblem(p: ParameterSpec): string | null {
  const nums = [p.min, p.max, p.step, p.default];
  if (nums.some((n) => !Number.isFinite(n))) return msg("Enter a number in each box.");
  if (!p.decimal && nums.some((n) => !Number.isInteger(n))) return msg("A whole number parameter takes whole numbers.");
  if (p.step <= 0) return msg("The increment must be more than 0.");
  if (p.max < p.min) return msg("The maximum must be at least the minimum.");
  if ((p.max - p.min) / p.step > 10_000) return msg("That's more than 10,000 values; use a larger increment.");
  if (p.default < p.min || p.default > p.max) return msg("The default must be between the minimum and the maximum.");
  return null;
}

/** The parameter's table (with its column and measure); `newId` makes their ids. */
export function parameterTable(p: ParameterSpec, newId: () => string): Table {
  const decimals = p.decimal ? Math.max(0, ...[p.min, p.step].map((n) => (String(n).split(".")[1] ?? "").length)) : 0;
  const column: Column = {
    id: newId(),
    name: p.name,
    // The series is computed as doubles; the column's type rounds them.
    dataType: p.decimal ? "Decimal" : "Int64",
    kind: { kind: "data", sourceColumn: "Value" },
    description: null,
    formatString: p.decimal ? `0.${"0".repeat(Math.max(1, decimals))}` : "0",
    summarizeBy: "None",
    dataCategory: null,
    sortBy: null,
    displayFolder: null,
    hidden: false,
    isKey: false,
    extras: {},
  };
  const ref = `${quoteTable(p.name)}[${p.name.replace(/]/g, "]]")}]`;
  const measure: Measure = {
    id: newId(),
    name: `${p.name} Value`,
    expression: `SELECTEDVALUE ( ${ref}, ${p.default} )`,
    language: "Dax",
    formatString: column.formatString,
    hidden: false,
  };
  const table: Table = {
    id: newId(),
    name: p.name,
    description: null,
    source: "",
    columns: [column],
    measures: [measure],
    hierarchies: [],
    hidden: false,
    isDateTable: false,
    calculated: { expression: `GENERATESERIES ( ${p.min}, ${p.max}, ${p.step} )`, language: "Dax" },
  };
  return table;
}

/** A field a field parameter offers: its label and its `'Table'[Name]` reference. */
export interface ParameterField {
  label: string;
  table: string;
  name: string;
}

/** A DAX string literal. */
const text = (s: string) => `"${s.replace(/"/g, '""')}"`;

/** `'Table'[Name]`, as NAMEOF returns it. */
export const fieldName = (table: string, name: string) => `'${table.replace(/'/g, "''")}'[${name.replace(/]/g, "]]")}]`;

/**
 * A field parameter's table, as Power BI writes one: rows of
 * `( label, NAMEOF ( field ), order )`, a label column named after the parameter and
 * sorted by the (hidden) order, and the hidden field names.
 */
export function fieldParameterTable(name: string, fields: ParameterField[], newId: () => string): Table {
  const column = (id: string, columnName: string, source: string, dataType: Column["dataType"], hidden: boolean, sortBy: string | null = null): Column => ({
    id,
    name: columnName,
    dataType,
    kind: { kind: "data", sourceColumn: source },
    description: null,
    formatString: null,
    summarizeBy: "None",
    dataCategory: null,
    sortBy,
    displayFolder: null,
    hidden,
    isKey: false,
    extras: {},
  });
  const order = column(newId(), `${name} Order`, "Value3", "Int64", true);
  const label = column(newId(), name, "Value1", "String", false, order.id);
  const names = column(newId(), `${name} Fields`, "Value2", "String", true);
  const rows = fields.map((f, i) => `    ( ${text(f.label)}, NAMEOF ( ${fieldName(f.table, f.name)} ), ${i} )`);
  return {
    id: newId(),
    name,
    description: null,
    source: "",
    columns: [label, names, order],
    measures: [],
    hierarchies: [],
    hidden: false,
    isDateTable: false,
    calculated: { expression: `{\n${rows.join(",\n")}\n}`, language: "Dax" },
  };
}
