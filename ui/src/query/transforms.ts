// The query editor's transforms: what each asks for and how it becomes a step. The
// toolbar, the column menus and the step "gear" (edit) all go through this list, and
// StepDialog renders the fields. Some steps look at the data when they are added (the
// header row, how many parts a split makes, a pivot's values): `build` asks the core.
import type { AggOp } from "../bridge/generated/AggOp";
import type { Aggregate } from "../bridge/generated/Aggregate";
import type { Analysis } from "../bridge/generated/Analysis";
import type { Condition } from "../bridge/generated/Condition";
import type { DataType } from "../bridge/generated/DataType";
import type { Query } from "../bridge/generated/Query";
import type { Rule } from "../bridge/generated/Rule";
import type { Step } from "../bridge/generated/Step";
import { msg } from "../i18n";

export type Field =
  | { key: string; label: string; type: "column" }
  | { key: string; label: string; type: "columns"; optional?: boolean }
  | { key: string; label: string; type: "text"; placeholder?: string; optional?: boolean }
  | { key: string; label: string; type: "number"; min?: number }
  | { key: string; label: string; type: "select"; options: { value: string; label: string }[] }
  | { key: string; label: string; type: "bool" }
  | { key: string; label: string; type: "formula" }
  | { key: string; label: string; type: "conditions" }
  | { key: string; label: string; type: "rules" }
  | { key: string; label: string; type: "aggregates" }
  | { key: string; label: string; type: "query"; multi?: boolean }
  | { key: string; label: string; type: "otherColumns"; of: string };

export type Values = Record<string, unknown>;

export interface BuildContext {
  /** Columns of the step the new one follows. */
  columns: string[];
  query: Query;
  queries: Query[];
  analyze(a: Analysis): Promise<unknown>;
}

export interface Transform {
  id: string;
  label: string;
  group: string;
  op: Step["op"];
  fields: Field[];
  /** Initial values (from an existing step when editing it, else from the column clicked). */
  init(step: Step | null, column: string | null): Values;
  build(v: Values, ctx: BuildContext): Promise<Step> | Step;
  /** Applies without a dialog when it has no fields to fill. */
  direct?: boolean;
}

export const TYPES: { value: DataType; label: string }[] = [
  { value: "String", label: msg("Text") },
  { value: "Int64", label: msg("Whole number") },
  { value: "Double", label: msg("Decimal number") },
  { value: "Decimal", label: msg("Fixed decimal number") },
  { value: "Date", label: msg("Date") },
  { value: "DateTime", label: msg("Date/Time") },
  { value: "Time", label: msg("Time") },
  { value: "Boolean", label: msg("True/False") },
];

export const OPS: { value: Condition["op"]; label: string }[] = [
  { value: "equals", label: msg("equals") },
  { value: "notEquals", label: msg("does not equal") },
  { value: "greater", label: msg("is greater than") },
  { value: "greaterOrEqual", label: msg("is greater than or equal to") },
  { value: "less", label: msg("is less than") },
  { value: "lessOrEqual", label: msg("is less than or equal to") },
  { value: "contains", label: msg("contains") },
  { value: "notContains", label: msg("does not contain") },
  { value: "startsWith", label: msg("begins with") },
  { value: "endsWith", label: msg("ends with") },
  { value: "isBlank", label: msg("is blank") },
  { value: "isNotBlank", label: msg("is not blank") },
];

export const AGGS: { value: AggOp; label: string }[] = [
  { value: "sum", label: msg("Sum") },
  { value: "average", label: msg("Average") },
  { value: "min", label: msg("Min") },
  { value: "max", label: msg("Max") },
  { value: "count", label: msg("Count values") },
  { value: "countDistinct", label: msg("Count distinct values") },
  { value: "countRows", label: msg("Count rows") },
];

const str = (v: unknown, d = "") => (typeof v === "string" ? v : d);
const num = (v: unknown, d = 0) => (typeof v === "number" && Number.isFinite(v) ? v : Number(v) || d);
const list = (v: unknown) => (Array.isArray(v) ? (v as string[]) : []);
const one = (c: string | null) => (c ? [c] : []);

/** Unique, non-blank column names from a header row (as Power Query names them). */
export function headerNames(row: (string | null)[]): string[] {
  const out: string[] = [];
  row.forEach((v, i) => {
    let n = (v ?? "").trim() || `Column${i + 1}`;
    for (let k = 2; out.includes(n); k++) n = `${(v ?? "").trim() || `Column${i + 1}`}_${k}`;
    out.push(n);
  });
  return out;
}

const step = <T extends Step["op"]>(s: Extract<Step, { op: T }>) => s;

export const TRANSFORMS: Transform[] = [
  // Manage columns
  {
    id: "chooseColumns",
    label: msg("Choose columns"),
    group: msg("Manage columns"),
    op: "selectColumns",
    fields: [{ key: "columns", label: msg("Columns to keep"), type: "columns" }],
    init: (s) => ({ columns: s?.op === "selectColumns" ? s.columns : [] }),
    build: (v) => step<"selectColumns">({ op: "selectColumns", columns: list(v.columns) }),
  },
  {
    id: "removeColumns",
    label: msg("Remove columns"),
    group: msg("Manage columns"),
    op: "removeColumns",
    fields: [{ key: "columns", label: msg("Columns to remove"), type: "columns" }],
    init: (s, c) => ({ columns: s?.op === "removeColumns" ? s.columns : one(c) }),
    build: (v) => step<"removeColumns">({ op: "removeColumns", columns: list(v.columns) }),
  },
  {
    id: "renameColumn",
    label: msg("Rename column"),
    group: msg("Manage columns"),
    op: "renameColumns",
    fields: [
      { key: "column", label: msg("Column"), type: "column" },
      { key: "name", label: msg("New name"), type: "text" },
    ],
    init: (s, c) => (s?.op === "renameColumns" ? { column: s.renames[0]?.[0], name: s.renames[0]?.[1] } : { column: c, name: c ?? "" }),
    build: (v) => step<"renameColumns">({ op: "renameColumns", renames: [[str(v.column), str(v.name).trim()]] }),
  },
  // Reduce rows
  {
    id: "keepTopRows",
    label: msg("Keep top rows"),
    group: msg("Reduce rows"),
    op: "keepTopRows",
    fields: [{ key: "count", label: msg("Number of rows"), type: "number", min: 1 }],
    init: (s) => ({ count: s?.op === "keepTopRows" ? s.count : 100 }),
    build: (v) => step<"keepTopRows">({ op: "keepTopRows", count: Math.max(1, Math.round(num(v.count, 1))) }),
  },
  {
    id: "removeTopRows",
    label: msg("Remove top rows"),
    group: msg("Reduce rows"),
    op: "removeTopRows",
    fields: [{ key: "count", label: msg("Number of rows"), type: "number", min: 1 }],
    init: (s) => ({ count: s?.op === "removeTopRows" ? s.count : 1 }),
    build: (v) => step<"removeTopRows">({ op: "removeTopRows", count: Math.max(1, Math.round(num(v.count, 1))) }),
  },
  {
    id: "removeDuplicates",
    label: msg("Remove duplicates"),
    group: msg("Reduce rows"),
    op: "removeDuplicates",
    fields: [{ key: "columns", label: msg("Compare these columns (all when none are chosen)"), type: "columns", optional: true }],
    init: (s, c) => ({ columns: s?.op === "removeDuplicates" ? s.columns : one(c) }),
    build: (v) => step<"removeDuplicates">({ op: "removeDuplicates", columns: list(v.columns) }),
  },
  {
    id: "removeBlankRows",
    label: msg("Remove blank rows"),
    group: msg("Reduce rows"),
    op: "removeBlankRows",
    fields: [],
    direct: true,
    init: () => ({}),
    build: () => ({ op: "removeBlankRows" }),
  },
  {
    id: "filterRows",
    label: msg("Filter rows"),
    group: msg("Reduce rows"),
    op: "filterRows",
    fields: [
      { key: "column", label: msg("Column"), type: "column" },
      { key: "conditions", label: msg("Keep rows where the column"), type: "conditions" },
    ],
    init: (s, c) =>
      s?.op === "filterRows" ? { column: s.column, conditions: s.conditions, or: s.or } : { column: c, conditions: [{ op: "equals", value: "", value2: null }], or: false },
    build: (v) => step<"filterRows">({ op: "filterRows", column: str(v.column), conditions: (v.conditions as Condition[]) ?? [], or: !!v.or }),
  },
  {
    id: "sortRows",
    label: msg("Sort rows"),
    group: msg("Reduce rows"),
    op: "sortRows",
    fields: [
      { key: "column", label: msg("Column"), type: "column" },
      { key: "order", label: msg("Order"), type: "select", options: [{ value: "asc", label: msg("Ascending") }, { value: "desc", label: msg("Descending") }] },
    ],
    init: (s, c) => (s?.op === "sortRows" ? { column: s.columns[0]?.[0], order: s.columns[0]?.[1] ? "desc" : "asc" } : { column: c, order: "asc" }),
    build: (v) => step<"sortRows">({ op: "sortRows", columns: [[str(v.column), v.order === "desc"]] }),
  },
  // Transform
  {
    id: "promoteHeaders",
    label: msg("Use first row as headers"),
    group: msg("Transform"),
    op: "promoteHeaders",
    fields: [],
    direct: true,
    init: () => ({}),
    build: async (_, ctx) => {
      const row = (await ctx.analyze({ kind: "firstRow" })) as (string | null)[];
      return { op: "promoteHeaders", names: headerNames(row) };
    },
  },
  {
    id: "changeType",
    label: msg("Change type"),
    group: msg("Transform"),
    op: "changeTypes",
    fields: [
      { key: "columns", label: msg("Columns"), type: "columns" },
      { key: "type", label: msg("Data type"), type: "select", options: TYPES },
    ],
    init: (s, c) => (s?.op === "changeTypes" ? { columns: s.columns.map((x) => x[0]), type: s.columns[0]?.[1] ?? "String" } : { columns: one(c), type: "String" }),
    build: (v) => step<"changeTypes">({ op: "changeTypes", columns: list(v.columns).map((c) => [c, str(v.type, "String") as DataType]) }),
  },
  {
    id: "replaceValues",
    label: msg("Replace values"),
    group: msg("Transform"),
    op: "replaceValues",
    fields: [
      { key: "columns", label: msg("Columns"), type: "columns" },
      { key: "find", label: msg("Value to find"), type: "text", optional: true, placeholder: msg("(blank)") },
      { key: "replace", label: msg("Replace with"), type: "text", optional: true },
      { key: "whole", label: msg("Match entire cell contents"), type: "bool" },
    ],
    init: (s, c) => (s?.op === "replaceValues" ? { ...s } : { columns: one(c), find: "", replace: "", whole: true }),
    build: (v) => step<"replaceValues">({ op: "replaceValues", columns: list(v.columns), find: str(v.find), replace: str(v.replace), whole: !!v.whole }),
  },
  {
    id: "splitColumn",
    label: msg("Split column by delimiter"),
    group: msg("Transform"),
    op: "splitColumn",
    fields: [
      { key: "column", label: msg("Column"), type: "column" },
      { key: "delimiter", label: msg("Delimiter"), type: "text" },
      {
        key: "mode",
        label: msg("Split at"),
        type: "select",
        options: [
          { value: "left", label: msg("Left-most delimiter") },
          { value: "right", label: msg("Right-most delimiter") },
          { value: "each", label: msg("Each occurrence of the delimiter") },
        ],
      },
    ],
    init: (s, c) => (s?.op === "splitColumn" ? { column: s.column, delimiter: s.delimiter, mode: s.mode } : { column: c, delimiter: ",", mode: "each" }),
    build: async (v, ctx) => {
      const column = str(v.column);
      const delimiter = str(v.delimiter, ",");
      const mode = str(v.mode, "each") as "left" | "right" | "each";
      let parts = 2;
      if (mode === "each") parts = Math.min(50, Math.max(1, Number(await ctx.analyze({ kind: "splitParts", column, delimiter })) || 1));
      return { op: "splitColumn", column, delimiter, mode, names: Array.from({ length: parts }, (_, i) => `${column}.${i + 1}`) };
    },
  },
  {
    id: "formatText",
    label: msg("Format text"),
    group: msg("Transform"),
    op: "formatText",
    fields: [
      { key: "columns", label: msg("Columns"), type: "columns" },
      {
        key: "format",
        label: msg("Format"),
        type: "select",
        options: [
          { value: "trim", label: msg("Trim") },
          { value: "clean", label: msg("Clean") },
          { value: "lower", label: msg("lowercase") },
          { value: "upper", label: msg("UPPERCASE") },
          { value: "capitalize", label: msg("Capitalize Each Word") },
        ],
      },
    ],
    init: (s, c) => (s?.op === "formatText" ? { columns: s.columns, format: s.format } : { columns: one(c), format: "trim" }),
    build: (v) => step<"formatText">({ op: "formatText", columns: list(v.columns), format: str(v.format, "trim") as "trim" }),
  },
  {
    id: "extractText",
    label: msg("Extract text"),
    group: msg("Transform"),
    op: "extractText",
    fields: [
      { key: "column", label: msg("Column"), type: "column" },
      {
        key: "kind",
        label: msg("Extract"),
        type: "select",
        options: [
          { value: "first", label: msg("First characters") },
          { value: "last", label: msg("Last characters") },
          { value: "range", label: msg("Range") },
          { value: "beforeDelimiter", label: msg("Text before delimiter") },
          { value: "afterDelimiter", label: msg("Text after delimiter") },
          { value: "length", label: msg("Length") },
        ],
      },
      { key: "count", label: msg("Number of characters"), type: "number", min: 1 },
      { key: "start", label: msg("Starting index (from 0)"), type: "number", min: 0 },
      { key: "delimiter", label: msg("Delimiter"), type: "text", optional: true },
      { key: "newColumn", label: msg("New column name (blank to replace the column)"), type: "text", optional: true },
    ],
    init: (s, c) => {
      if (s?.op !== "extractText") return { column: c, kind: "first", count: 3, start: 0, delimiter: "", newColumn: "" };
      const e = s.extract;
      return {
        column: s.column,
        kind: e.kind,
        count: "count" in e ? e.count : 3,
        start: "start" in e ? e.start : 0,
        delimiter: "delimiter" in e ? e.delimiter : "",
        newColumn: s.newColumn ?? "",
      };
    },
    build: (v) => {
      const count = Math.max(1, Math.round(num(v.count, 1)));
      const delimiter = str(v.delimiter);
      const kind = str(v.kind, "first");
      const extract =
        kind === "last"
          ? { kind: "last" as const, count }
          : kind === "range"
            ? { kind: "range" as const, start: Math.max(0, Math.round(num(v.start))), count }
            : kind === "beforeDelimiter"
              ? { kind: "beforeDelimiter" as const, delimiter }
              : kind === "afterDelimiter"
                ? { kind: "afterDelimiter" as const, delimiter }
                : kind === "length"
                  ? { kind: "length" as const }
                  : { kind: "first" as const, count };
      return { op: "extractText", column: str(v.column), extract, newColumn: str(v.newColumn).trim() || null };
    },
  },
  {
    id: "fillDown",
    label: msg("Fill down"),
    group: msg("Transform"),
    op: "fillDown",
    fields: [{ key: "columns", label: msg("Columns"), type: "columns" }],
    init: (s, c) => ({ columns: s?.op === "fillDown" ? s.columns : one(c) }),
    build: (v) => step<"fillDown">({ op: "fillDown", columns: list(v.columns) }),
  },
  {
    id: "unpivot",
    label: msg("Unpivot columns"),
    group: msg("Transform"),
    op: "unpivot",
    fields: [
      { key: "columns", label: msg("Columns to unpivot"), type: "columns" },
      { key: "attribute", label: msg("Attribute column name"), type: "text" },
      { key: "value", label: msg("Value column name"), type: "text" },
    ],
    init: (s, c) => (s?.op === "unpivot" ? { ...s } : { columns: one(c), attribute: "Attribute", value: "Value" }),
    build: (v) => step<"unpivot">({ op: "unpivot", columns: list(v.columns), attribute: str(v.attribute, "Attribute"), value: str(v.value, "Value") }),
  },
  {
    id: "unpivotOthers",
    label: msg("Unpivot other columns"),
    group: msg("Transform"),
    op: "unpivot",
    fields: [{ key: "keep", label: msg("Columns to keep"), type: "columns" }],
    init: (_, c) => ({ keep: one(c) }),
    build: (v, ctx) => step<"unpivot">({ op: "unpivot", columns: ctx.columns.filter((c) => !list(v.keep).includes(c)), attribute: "Attribute", value: "Value" }),
  },
  {
    id: "pivot",
    label: msg("Pivot column"),
    group: msg("Transform"),
    op: "pivot",
    fields: [
      { key: "column", label: msg("Column whose values become columns"), type: "column" },
      { key: "valuesColumn", label: msg("Values column"), type: "column" },
      { key: "aggregate", label: msg("Aggregate values with"), type: "select", options: AGGS.filter((a) => a.value !== "countRows") },
    ],
    init: (s, c) => (s?.op === "pivot" ? { ...s } : { column: c, valuesColumn: null, aggregate: "sum" }),
    build: async (v, ctx) => {
      const column = str(v.column);
      const values = (await ctx.analyze({ kind: "distinct", column, limit: 200 })) as string[];
      return { op: "pivot", column, valuesColumn: str(v.valuesColumn), values, aggregate: str(v.aggregate, "sum") as AggOp };
    },
  },
  {
    id: "groupBy",
    label: msg("Group by"),
    group: msg("Transform"),
    op: "groupBy",
    fields: [
      { key: "keys", label: msg("Group by"), type: "columns" },
      { key: "aggregates", label: msg("New columns"), type: "aggregates" },
    ],
    init: (s, c) => (s?.op === "groupBy" ? { keys: s.keys, aggregates: s.aggregates } : { keys: one(c), aggregates: [{ name: "Count", op: "countRows", column: null }] }),
    build: (v) => step<"groupBy">({ op: "groupBy", keys: list(v.keys), aggregates: (v.aggregates as Aggregate[]) ?? [] }),
  },
  // Add column
  {
    id: "customColumn",
    label: msg("Custom column"),
    group: msg("Add column"),
    op: "customColumn",
    fields: [
      { key: "name", label: msg("New column name"), type: "text" },
      { key: "formula", label: msg("Custom column formula"), type: "formula" },
    ],
    init: (s) => (s?.op === "customColumn" ? { name: s.name, formula: s.formula } : { name: "Custom", formula: "" }),
    build: (v) => step<"customColumn">({ op: "customColumn", name: str(v.name, "Custom").trim(), formula: str(v.formula) }),
  },
  {
    id: "conditionalColumn",
    label: msg("Conditional column"),
    group: msg("Add column"),
    op: "conditionalColumn",
    fields: [
      { key: "name", label: msg("New column name"), type: "text" },
      { key: "rules", label: msg("Rules"), type: "rules" },
      { key: "otherwise", label: msg("Else"), type: "text", optional: true },
    ],
    init: (s, c) =>
      s?.op === "conditionalColumn"
        ? { name: s.name, rules: s.rules, otherwise: s.otherwise ?? "" }
        : { name: "Custom", rules: [{ column: c ?? "", op: "equals", value: "", output: "" }], otherwise: "" },
    build: (v) =>
      step<"conditionalColumn">({ op: "conditionalColumn", name: str(v.name, "Custom").trim(), rules: (v.rules as Rule[]) ?? [], otherwise: str(v.otherwise) || null }),
  },
  {
    id: "indexColumn",
    label: msg("Index column"),
    group: msg("Add column"),
    op: "addIndex",
    fields: [
      { key: "name", label: msg("New column name"), type: "text" },
      { key: "start", label: msg("Starting index"), type: "number" },
      { key: "step", label: msg("Increment"), type: "number" },
    ],
    init: (s) => (s?.op === "addIndex" ? { ...s } : { name: "Index", start: 1, step: 1 }),
    build: (v) => step<"addIndex">({ op: "addIndex", name: str(v.name, "Index"), start: Math.round(num(v.start)), step: Math.round(num(v.step, 1)) || 1 }),
  },
  {
    id: "mergeColumns",
    label: msg("Merge columns"),
    group: msg("Add column"),
    op: "mergeColumns",
    fields: [
      { key: "columns", label: msg("Columns, in order"), type: "columns" },
      { key: "separator", label: msg("Separator"), type: "text", optional: true },
      { key: "name", label: msg("New column name"), type: "text" },
    ],
    init: (s, c) => (s?.op === "mergeColumns" ? { ...s } : { columns: one(c), separator: " ", name: "Merged" }),
    build: (v) => step<"mergeColumns">({ op: "mergeColumns", columns: list(v.columns), separator: str(v.separator), name: str(v.name, "Merged") }),
  },
  // Combine
  {
    id: "mergeQueries",
    label: msg("Merge queries"),
    group: msg("Combine"),
    op: "mergeQueries",
    fields: [
      { key: "left", label: msg("Matching columns in this query"), type: "columns" },
      { key: "query", label: msg("Merge with"), type: "query" },
      { key: "right", label: msg("Matching columns in that query"), type: "otherColumns", of: "query" },
      {
        key: "kind",
        label: msg("Join kind"),
        type: "select",
        options: [
          { value: "leftOuter", label: msg("Left outer (all from first, matching from second)") },
          { value: "inner", label: msg("Inner (only matching rows)") },
          { value: "fullOuter", label: msg("Full outer (all rows from both)") },
          { value: "rightOuter", label: msg("Right outer (all from second, matching from first)") },
          { value: "leftAnti", label: msg("Left anti (rows only in first)") },
        ],
      },
      { key: "columns", label: msg("Columns to add"), type: "otherColumns", of: "query" },
      { key: "prefix", label: msg("Prefix for the added columns"), type: "text", optional: true },
    ],
    init: (s, c) => (s?.op === "mergeQueries" ? { ...s, prefix: s.prefix ?? "" } : { left: one(c), query: null, right: [], kind: "leftOuter", columns: [], prefix: "" }),
    build: (v) =>
      step<"mergeQueries">({
        op: "mergeQueries",
        query: str(v.query),
        left: list(v.left),
        right: list(v.right),
        kind: str(v.kind, "leftOuter") as "leftOuter",
        columns: list(v.columns),
        prefix: str(v.prefix).trim() || null,
      }),
  },
  {
    id: "appendQueries",
    label: msg("Append queries"),
    group: msg("Combine"),
    op: "appendQueries",
    fields: [{ key: "queries", label: msg("Queries to append"), type: "query", multi: true }],
    init: (s) => ({ queries: s?.op === "appendQueries" ? s.queries : [] }),
    build: (v) => step<"appendQueries">({ op: "appendQueries", queries: list(v.queries) }),
  },
];

export const transformById = (id: string) => TRANSFORMS.find((t) => t.id === id);

/** The transform that edits an existing step (its gear). */
export function transformFor(s: Step): Transform | undefined {
  if (s.op === "unpivot") return transformById("unpivot");
  return TRANSFORMS.find((t) => t.op === s.op);
}

/** What a step needs filled in before it can be built. */
export function missing(t: Transform, v: Values): string | null {
  for (const f of t.fields) {
    const x = v[f.key];
    if ("optional" in f && f.optional) continue;
    if ((f.type === "columns" || f.type === "otherColumns" || (f.type === "query" && f.multi)) && list(x).length === 0) return f.label;
    if ((f.type === "column" || f.type === "query") && !x) return f.label;
    if ((f.type === "text" || f.type === "formula") && !str(x).trim() && !(f.key === "delimiter" && t.id !== "splitColumn")) return f.label;
  }
  if (t.id === "mergeQueries" && list(v.left).length !== list(v.right).length) return msg("Matching columns in that query");
  return null;
}

/** Power Query's step names (as the core's `Step::default_name`). */
const STEP_NAMES: Record<Step["op"], string> = {
  removeTopRows: "Removed Top Rows",
  keepTopRows: "Kept First Rows",
  promoteHeaders: "Promoted Headers",
  changeTypes: "Changed Type",
  selectColumns: "Removed Other Columns",
  removeColumns: "Removed Columns",
  renameColumns: "Renamed Columns",
  filterRows: "Filtered Rows",
  removeDuplicates: "Removed Duplicates",
  removeBlankRows: "Removed Blank Rows",
  replaceValues: "Replaced Value",
  splitColumn: "Split Column by Delimiter",
  mergeColumns: "Merged Columns",
  customColumn: "Added Custom",
  conditionalColumn: "Added Conditional Column",
  groupBy: "Grouped Rows",
  unpivot: "Unpivoted Columns",
  pivot: "Pivoted Column",
  fillDown: "Filled Down",
  addIndex: "Added Index",
  extractText: "Extracted Text",
  formatText: "Formatted Text",
  sortRows: "Sorted Rows",
  mergeQueries: "Merged Queries",
  appendQueries: "Appended Query",
};

/** Display names of the source and each step, unique as Power Query makes them (as
 * the core's `Query::step_display_names`). */
export function stepDisplayNames(q: Query): string[] {
  const out = ["Source"];
  q.steps.forEach((s, i) => {
    const base = (q.stepNames ?? {})[i] ?? STEP_NAMES[s.op];
    let name = base;
    for (let k = 2; out.includes(name); k++) name = `${base} ${k}`;
    out.push(name);
  });
  return out;
}
