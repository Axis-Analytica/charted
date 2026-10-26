// Groups and bins (Power BI's "New group"): calculated columns whose DAX is written from
// the group dialog's settings. The settings stay in the column's extras
// (`charted.group`) so the dialog can reopen them; DAX alone is what evaluates.
import type { Column } from "../bridge/generated/Column";
import type { DataType } from "../bridge/generated/DataType";

/** Bins of numbers (`number`) or of dates (days, months or years). */
export type BinUnit = "number" | "day" | "month" | "year";

export interface BinGroup {
  kind: "bin";
  /** The grouped column's id. */
  column: string;
  unit: BinUnit;
  /** Bin size, in the unit. */
  size: number;
  /** How the size was chosen: directly, or from a number of bins. */
  by: "size" | "count";
  count: number;
}

export interface ListGroup {
  kind: "list";
  column: string;
  groups: { name: string; values: string[] }[];
  /** The name of the group for every other value, or null to keep those values as they are. */
  other: string | null;
}

export type GroupSpec = BinGroup | ListGroup;

export const isNumericType = (t: DataType) => t === "Int64" || t === "Double" || t === "Decimal";
export const isDateType = (t: DataType) => t === "Date" || t === "DateTime" || t === "DateTimeTz";

/** The kind of group a column takes: bins for numbers and dates, lists for the rest. */
export const groupKind = (t: DataType): "bin" | "list" => (isNumericType(t) || isDateType(t) ? "bin" : "list");

const quote = (s: string) => `"${s.replace(/"/g, '""')}"`;
export const columnRef = (table: string, column: string) => `'${table.replace(/'/g, "''")}'[${column.replace(/]/g, "]]")}]`;

/** A number as DAX writes it (no exponent, no locale). */
function num(n: number): string {
  if (Number.isInteger(n)) return String(n);
  return String(Number(n.toPrecision(12)));
}

/** Decimal places in a bin size, so sums of floating-point steps round cleanly. */
function decimals(n: number): number {
  const s = num(n);
  const dot = s.indexOf(".");
  return dot < 0 ? 0 : s.length - dot - 1;
}

/** The group's DAX: SWITCH with IN for lists, INT arithmetic for bins. */
export function groupDax(spec: GroupSpec, table: string, column: string, type: DataType): string {
  const c = columnRef(table, column);
  if (spec.kind === "list") {
    const value = (v: string) => (isNumericType(type) ? v : quote(v));
    const arms = spec.groups.filter((g) => g.values.length).map((g) => `    ${c} IN { ${g.values.map(value).join(", ")} }, ${quote(g.name)}`);
    const rest = spec.other === null ? c : quote(spec.other);
    if (!arms.length) return rest;
    return `SWITCH (\n    TRUE (),\n${arms.join(",\n")},\n    ${rest}\n)`;
  }
  const n = Math.max(1, spec.unit === "number" ? spec.size : Math.round(spec.size));
  const bin = (() => {
    switch (spec.unit) {
      case "number": {
        const b = `INT ( ${c} / ${num(spec.size)} ) * ${num(spec.size)}`;
        return decimals(spec.size) ? `ROUND ( ${b}, ${decimals(spec.size)} )` : b;
      }
      case "year":
        return `DATE ( INT ( YEAR ( ${c} ) / ${n} ) * ${n}, 1, 1 )`;
      case "month":
        return `DATE ( 1900, INT ( ( ( YEAR ( ${c} ) - 1900 ) * 12 + MONTH ( ${c} ) - 1 ) / ${n} ) * ${n} + 1, 1 )`;
      case "day":
        return `DATE ( 1900, 1, 1 + INT ( DATEDIFF ( DATE ( 1900, 1, 1 ), ${c}, DAY ) / ${n} ) * ${n} )`;
    }
  })();
  return `IF ( ISBLANK ( ${c} ), BLANK (), ${bin} )`;
}

/** A readable bin size from a number of bins over the column's range. */
export function sizeForCount(unit: BinUnit, count: number, min: number, max: number, integer: boolean): number {
  const span = Math.max(0, max - min);
  const raw = span / Math.max(1, count);
  if (unit !== "number" || integer) return Math.max(1, Math.ceil(raw));
  if (raw <= 0) return 1;
  // Two significant digits, rounded up so the bins cover the range.
  const p = 10 ** (Math.floor(Math.log10(raw)) - 1);
  return Number((Math.ceil(raw / p) * p).toPrecision(12));
}

/** The default name of a group column, as Power BI names them. */
export const groupName = (column: string, kind: "bin" | "list") => (kind === "bin" ? `${column} (bins)` : `${column} (groups)`);

/** A list group's name from its values ("A & B"), as Power BI names a new group. */
export const listGroupName = (values: string[]) => values.join(" & ");

/** The group settings stored on a column, if it was made by the group dialog. */
export function groupOf(column: Column): GroupSpec | null {
  const charted = column.extras?.charted as Record<string, unknown> | undefined;
  const g = charted?.group as GroupSpec | undefined;
  return g && (g.kind === "bin" || g.kind === "list") ? g : null;
}

/** The column's extras with the group settings in place. */
export function withGroup(extras: Column["extras"], spec: GroupSpec): Record<string, unknown> {
  const charted = { ...((extras?.charted as Record<string, unknown> | undefined) ?? {}), group: spec };
  return { ...(extras ?? {}), charted };
}
