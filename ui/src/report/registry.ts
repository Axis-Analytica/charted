// The capability registry: what each visual type accepts (data roles), how it can be
// formatted (objects → generated format pane) and how big it starts. Every pane reads
// this, so adding a visual type is one entry plus its renderer.
import { msg } from "../i18n";
import {
  ChartBarBig,
  ChartBarStacked,
  ChartColumnBig,
  ChartColumnStacked,
  ChartLine,
  ChartPie,
  ChartScatter,
  CreditCard,
  Disc,
  Grid3x3,
  Image,
  ListFilter,
  Shapes,
  Table2,
  Type,
  type LucideIcon,
} from "lucide-react";

export type RoleKind = "grouping" | "value" | "any";

export interface RoleDef {
  name: string;
  /** English label; translated when shown. */
  label: string;
  kind: RoleKind;
  max?: number;
  /** How many fields the role takes when they are added without a target (a ticked
   * field); more can be dragged in (an axis hierarchy). */
  autoMax?: number;
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
  /** Shows no data (text, images, shapes): no fields, no interactions. */
  decor?: boolean;
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
  { name: "category", label: axisLabel, kind: "grouping", autoMax: 1 },
  { name: "series", label: "Legend", kind: "grouping", max: 1 },
  { name: "values", label: "Values", kind: "value" },
];

const pieRoles: RoleDef[] = [
  { name: "legend", label: "Legend", kind: "grouping" },
  { name: "values", label: "Values", kind: "value", max: 1 },
];

const pieLabels: ObjectDef = {
  name: "dataLabels",
  label: "Detail labels",
  props: [
    {
      name: "content",
      label: "Label contents",
      type: "select",
      default: "category",
      options: [
        { value: "category", label: "Category, percent of total" },
        { value: "percent", label: "Percent of total" },
        { value: "value", label: "Data value" },
        { value: "none", label: "None" },
      ],
    },
  ],
};

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
    type: "stackedColumnChart",
    label: "Stacked column chart",
    icon: ChartColumnStacked,
    roles: cartesianRoles(msg("X-axis")),
    objects: [title, legend, axis("categoryAxis", msg("X-axis")), axis("valueAxis", msg("Y-axis")), dataLabels, colors, background],
    size: { width: 480, height: 300 },
  },
  {
    type: "stackedBarChart",
    label: "Stacked bar chart",
    icon: ChartBarStacked,
    roles: cartesianRoles(msg("Y-axis")),
    objects: [title, legend, axis("categoryAxis", msg("Y-axis")), axis("valueAxis", msg("X-axis")), dataLabels, colors, background],
    size: { width: 480, height: 300 },
  },
  {
    type: "pieChart",
    label: "Pie chart",
    icon: ChartPie,
    roles: pieRoles,
    objects: [title, { ...legend, props: legend.props.map((p) => (p.name === "position" ? { ...p, default: "right" } : p)) } as ObjectDef, pieLabels, background],
    size: { width: 360, height: 280 },
  },
  {
    type: "donutChart",
    label: "Donut chart",
    icon: Disc,
    roles: pieRoles,
    objects: [title, { ...legend, props: legend.props.map((p) => (p.name === "position" ? { ...p, default: "right" } : p)) } as ObjectDef, pieLabels, background],
    size: { width: 360, height: 280 },
  },
  {
    type: "scatterChart",
    label: "Scatter chart",
    icon: ChartScatter,
    roles: [
      { name: "details", label: "Values", kind: "grouping", max: 1 },
      { name: "legend", label: "Legend", kind: "grouping", max: 1 },
      { name: "x", label: "X-axis", kind: "value", max: 1 },
      { name: "y", label: "Y-axis", kind: "value", max: 1 },
      { name: "size", label: "Size", kind: "value", max: 1 },
    ],
    objects: [
      title,
      legend,
      {
        name: "axes",
        label: "Axes",
        props: [
          { name: "titles", label: "Show titles", type: "bool", default: true },
          { name: "gridlines", label: "Gridlines", type: "bool", default: true },
        ],
      },
      { name: "markers", label: "Markers", props: [{ name: "size", label: "Size", type: "number", default: 10, min: 4, max: 40 }] },
      background,
    ],
    size: { width: 480, height: 320 },
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

VISUALS.push(
  {
    type: "matrix",
    label: "Matrix",
    icon: Grid3x3,
    roles: [
      { name: "rows", label: "Rows", kind: "grouping" },
      { name: "columns", label: "Columns", kind: "grouping" },
      { name: "values", label: "Values", kind: "value" },
    ],
    objects: [
      title,
      {
        name: "grid",
        label: "Grid",
        props: [
          { name: "fontSize", label: "Text size", type: "number", default: 12, min: 8, max: 24 },
          { name: "banded", label: "Banded rows", type: "bool", default: false },
        ],
      },
      {
        name: "totals",
        label: "Totals",
        props: [
          { name: "rows", label: "Row totals", type: "bool", default: true },
          { name: "columns", label: "Column totals", type: "bool", default: true },
          { name: "subtotals", label: "Subtotals", type: "bool", default: true },
        ],
      },
      background,
    ],
    size: { width: 520, height: 340 },
  },
  {
    type: "slicer",
    label: "Slicer",
    icon: ListFilter,
    roles: [{ name: "field", label: "Field", kind: "grouping", max: 1 }],
    objects: [
      {
        name: "slicer",
        label: "Slicer settings",
        props: [
          {
            name: "style",
            label: "Style",
            type: "select",
            default: "list",
            options: [
              { value: "list", label: "Vertical list" },
              { value: "dropdown", label: "Dropdown" },
              { value: "between", label: "Between" },
              { value: "relative", label: "Relative date" },
            ],
          },
        ],
      },
      {
        name: "selection",
        label: "Selection",
        props: [
          { name: "multi", label: "Multi-select without Ctrl", type: "bool", default: false },
          { name: "selectAll", label: "Show \"Select all\"", type: "bool", default: true },
          { name: "search", label: "Search", type: "bool", default: false },
        ],
      },
      { name: "header", label: "Slicer header", toggle: "show", props: [{ name: "show", label: "Show", type: "bool", default: true }] },
      { ...title, props: title.props.map((p) => (p.name === "show" ? { ...p, default: false } : p)) } as ObjectDef,
      background,
    ],
    size: { width: 200, height: 240 },
  },
  {
    type: "textbox",
    label: "Text box",
    icon: Type,
    roles: [],
    decor: true,
    objects: [
      {
        name: "text",
        label: "Text",
        props: [
          { name: "content", label: "Text", type: "text", default: "" },
          { name: "fontSize", label: "Text size", type: "number", default: 16, min: 8, max: 96 },
          { name: "bold", label: "Bold", type: "bool", default: false },
          { name: "align", label: "Alignment", type: "select", default: "left", options: [{ value: "left", label: "Left" }, { value: "center", label: "Center" }, { value: "right", label: "Right" }] },
          { name: "color", label: "Color", type: "color", default: "" },
        ],
      },
      { ...background, props: background.props.map((p) => ({ ...p, default: false })) } as ObjectDef,
    ],
    size: { width: 320, height: 64 },
  },
  {
    type: "image",
    label: "Image",
    icon: Image,
    roles: [],
    decor: true,
    objects: [
      {
        name: "image",
        label: "Image",
        props: [
          { name: "fit", label: "Scaling", type: "select", default: "contain", options: [{ value: "contain", label: "Fit" }, { value: "cover", label: "Fill" }, { value: "fill", label: "Stretch" }] },
          { name: "alt", label: "Alt text", type: "text", default: "" },
        ],
      },
      { ...background, props: background.props.map((p) => ({ ...p, default: false })) } as ObjectDef,
    ],
    size: { width: 240, height: 160 },
  },
  {
    type: "shape",
    label: "Shape",
    icon: Shapes,
    roles: [],
    decor: true,
    objects: [
      {
        name: "shape",
        label: "Shape",
        props: [
          {
            name: "kind",
            label: "Shape",
            type: "select",
            default: "roundedRectangle",
            options: [
              { value: "rectangle", label: "Rectangle" },
              { value: "roundedRectangle", label: "Rounded rectangle" },
              { value: "ellipse", label: "Oval" },
              { value: "line", label: "Line" },
              { value: "arrow", label: "Arrow" },
            ],
          },
          { name: "fill", label: "Fill", type: "color", default: "" },
          { name: "outline", label: "Outline", type: "color", default: "" },
          { name: "weight", label: "Outline width", type: "number", default: 2, min: 0, max: 20 },
        ],
      },
    ],
    size: { width: 200, height: 120 },
  },
);

export const visualDef = (type: string): VisualDef | undefined => VISUALS.find((v) => v.type === type);

/** A formatting property's value, falling back to the registry default. */
export function prop<T>(objects: Record<string, Record<string, unknown>> | undefined, def: VisualDef | undefined, object: string, name: string): T {
  const set = objects?.[object]?.[name];
  if (set !== undefined) return set as T;
  const p = def?.objects.find((o) => o.name === object)?.props.find((x) => x.name === name);
  return p?.default as T;
}
