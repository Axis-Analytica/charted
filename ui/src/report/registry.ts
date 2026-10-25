// The capability registry: what each visual type accepts (data roles), how it can be
// formatted (objects → generated format pane) and how big it starts. Every pane reads
// this, so adding a visual type is one entry plus its renderer.
import { msg } from "../i18n";
import { ChartBarBig, ChartColumnBig, ChartLine, CreditCard, Table2, type LucideIcon } from "lucide-react";

export type RoleKind = "grouping" | "value" | "any";

export interface RoleDef {
  name: string;
  /** English label; translated when shown. */
  label: string;
  kind: RoleKind;
  max?: number;
}

export type PropDef =
  | { name: string; label: string; type: "bool"; default: boolean }
  | { name: string; label: string; type: "text"; default: string; placeholder?: string }
  | { name: string; label: string; type: "number"; default: number; min?: number; max?: number; step?: number }
  | { name: string; label: string; type: "select"; default: string; options: { value: string; label: string }[] }
  | { name: string; label: string; type: "color"; default: string };

export interface ObjectDef {
  name: string;
  label: string;
  /** A boolean prop shown as the section's own switch. */
  toggle?: string;
  props: PropDef[];
}

export interface VisualDef {
  type: string;
  label: string;
  icon: LucideIcon;
  roles: RoleDef[];
  objects: ObjectDef[];
  size: { width: number; height: number };
}

const title: ObjectDef = {
  name: "title",
  label: "Title",
  toggle: "show",
  props: [
    { name: "show", label: "Show", type: "bool", default: true },
    { name: "text", label: "Text", type: "text", default: "", placeholder: msg("Automatic") },
    { name: "fontSize", label: "Text size", type: "number", default: 13, min: 8, max: 40 },
    { name: "align", label: "Alignment", type: "select", default: "left", options: [{ value: "left", label: "Left" }, { value: "center", label: "Center" }, { value: "right", label: "Right" }] },
  ],
};

const background: ObjectDef = {
  name: "background",
  label: "Background",
  toggle: "show",
  props: [
    { name: "show", label: "Show", type: "bool", default: true },
    { name: "border", label: "Border", type: "bool", default: true },
  ],
};

const legend: ObjectDef = {
  name: "legend",
  label: "Legend",
  toggle: "show",
  props: [
    { name: "show", label: "Show", type: "bool", default: true },
    { name: "position", label: "Position", type: "select", default: "top", options: [{ value: "top", label: "Top" }, { value: "bottom", label: "Bottom" }, { value: "right", label: "Right" }] },
  ],
};

const axis = (name: "categoryAxis" | "valueAxis", label: string): ObjectDef => ({
  name,
  label,
  toggle: "show",
  props: [
    { name: "show", label: "Show", type: "bool", default: true },
    { name: "title", label: "Show title", type: "bool", default: false },
    { name: "gridlines", label: "Gridlines", type: "bool", default: name === "valueAxis" },
  ],
});

const dataLabels: ObjectDef = {
  name: "dataLabels",
  label: "Data labels",
  toggle: "show",
  props: [
    { name: "show", label: "Show", type: "bool", default: false },
    { name: "fontSize", label: "Text size", type: "number", default: 11, min: 8, max: 24 },
  ],
};

const colors: ObjectDef = {
  name: "colors",
  label: "Colors",
  props: [{ name: "fill", label: "Default color", type: "color", default: "" }],
};

const cartesianRoles = (axisLabel: string): RoleDef[] => [
  { name: "category", label: axisLabel, kind: "grouping", max: 1 },
  { name: "series", label: "Legend", kind: "grouping", max: 1 },
  { name: "values", label: "Values", kind: "value" },
];

export const VISUALS: VisualDef[] = [
  {
    type: "card",
    label: "Card",
    icon: CreditCard,
    roles: [{ name: "values", label: "Fields", kind: "value", max: 1 }],
    objects: [
      {
        name: "callout",
        label: "Callout value",
        props: [
          { name: "fontSize", label: "Text size", type: "number", default: 36, min: 12, max: 96 },
          { name: "displayUnits", label: "Display units", type: "select", default: "auto", options: [{ value: "auto", label: "Auto" }, { value: "none", label: "None" }, { value: "K", label: "Thousands" }, { value: "M", label: "Millions" }, { value: "B", label: "Billions" }] },
        ],
      },
      { name: "label", label: "Category label", toggle: "show", props: [{ name: "show", label: "Show", type: "bool", default: true }] },
      { ...title, props: title.props.map((p) => (p.name === "show" ? { ...p, default: false } : p)) } as ObjectDef,
      background,
    ],
    size: { width: 240, height: 140 },
  },
  {
    type: "columnChart",
    label: "Clustered column chart",
    icon: ChartColumnBig,
    roles: cartesianRoles(msg("X-axis")),
    objects: [title, legend, axis("categoryAxis", msg("X-axis")), axis("valueAxis", msg("Y-axis")), dataLabels, colors, background],
    size: { width: 480, height: 300 },
  },
  {
    type: "barChart",
    label: "Clustered bar chart",
    icon: ChartBarBig,
    roles: cartesianRoles(msg("Y-axis")),
    objects: [title, legend, axis("categoryAxis", msg("Y-axis")), axis("valueAxis", msg("X-axis")), dataLabels, colors, background],
    size: { width: 480, height: 300 },
  },
  {
    type: "lineChart",
    label: "Line chart",
    icon: ChartLine,
    roles: cartesianRoles(msg("X-axis")),
    objects: [
      title,
      legend,
      axis("categoryAxis", msg("X-axis")),
      axis("valueAxis", msg("Y-axis")),
      {
        name: "lines",
        label: "Lines",
        props: [
          { name: "width", label: "Width", type: "number", default: 2, min: 1, max: 8 },
          { name: "markers", label: "Markers", type: "bool", default: false },
          { name: "smooth", label: "Smooth", type: "bool", default: false },
        ],
      },
      dataLabels,
      colors,
      background,
    ],
    size: { width: 560, height: 300 },
  },
  {
    type: "table",
    label: "Table",
    icon: Table2,
    roles: [{ name: "columns", label: "Columns", kind: "any" }],
    objects: [
      title,
      {
        name: "grid",
        label: "Grid",
        props: [
          { name: "fontSize", label: "Text size", type: "number", default: 12, min: 8, max: 24 },
          { name: "rowLines", label: "Row lines", type: "bool", default: true },
          { name: "banded", label: "Banded rows", type: "bool", default: false },
        ],
      },
      { name: "totals", label: "Totals", toggle: "show", props: [{ name: "show", label: "Show", type: "bool", default: true }] },
      background,
    ],
    size: { width: 480, height: 320 },
  },
];

export const visualDef = (type: string): VisualDef | undefined => VISUALS.find((v) => v.type === type);

/** A formatting property's value, falling back to the registry default. */
export function prop<T>(objects: Record<string, Record<string, unknown>> | undefined, def: VisualDef | undefined, object: string, name: string): T {
  const set = objects?.[object]?.[name];
  if (set !== undefined) return set as T;
  const p = def?.objects.find((o) => o.name === object)?.props.find((x) => x.name === name);
  return p?.default as T;
}
