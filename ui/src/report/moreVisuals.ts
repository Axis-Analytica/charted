// The v1.2 visual types (100% stacked, combo, waterfall, funnel, treemap, gauge, KPI,
// multi-row card), and what they add to the cartesian charts (small multiples and
// analytics lines). Kept apart from registry.ts, which calls `extendRegistry` once with
// its shared format objects.
import {
  ChartCandlestick,
  ChartColumnIncreasing,
  ChartNoAxesCombined,
  Columns4,
  Funnel,
  Gauge,
  LayoutDashboard,
  Rows3,
  Rows4,
  TrendingUp,
} from "lucide-react";
import { msg } from "../i18n";
import type { ObjectDef, RoleDef, VisualDef } from "./registry";

export interface Blocks {
  title: ObjectDef;
  background: ObjectDef;
  legend: ObjectDef;
  axis(name: "categoryAxis" | "valueAxis", label: string): ObjectDef;
  dataLabels: ObjectDef;
  colors: ObjectDef;
  cartesianRoles(axisLabel: string): RoleDef[];
}

/** Column, bar and line charts that can be split into small multiples. */
export const MULTIPLES = ["columnChart", "barChart", "lineChart", "stackedColumnChart", "stackedBarChart", "hundredPercentStackedColumnChart", "hundredPercentStackedBarChart"];

/** Charts with an Analytics section (constant, average, min, max and trend lines). */
export const ANALYTICS = ["columnChart", "barChart", "lineChart", "stackedColumnChart", "stackedBarChart", "lineClusteredColumnComboChart", "lineStackedColumnComboChart"];

const smallMultiplesRole: RoleDef = { name: "smallMultiples", label: "Small multiples", kind: "grouping", max: 1 };

export const analytics: ObjectDef = {
  name: "analytics",
  label: "Analytics",
  props: [
    { name: "constant", label: "Constant line", type: "bool", default: false },
    { name: "constantValue", label: "Constant line value", type: "number", default: 0, step: 1 },
    { name: "average", label: "Average line", type: "bool", default: false },
    { name: "min", label: "Min line", type: "bool", default: false },
    { name: "max", label: "Max line", type: "bool", default: false },
    { name: "trend", label: "Trend line", type: "bool", default: false },
    { name: "labels", label: "Line labels", type: "bool", default: true },
  ],
};

const smallMultiples: ObjectDef = {
  name: "smallMultiples",
  label: "Small multiples",
  props: [
    { name: "columns", label: "Columns", type: "number", default: 3, min: 1, max: 6 },
    { name: "sharedY", label: "Shared y-axis", type: "bool", default: true },
  ],
};

export const tooltipObject: ObjectDef = {
  name: "tooltip",
  label: "Tooltips",
  editor: "tooltipPage",
  props: [],
};

const callout = (size: number): ObjectDef => ({
  name: "callout",
  label: "Callout value",
  props: [
    { name: "fontSize", label: "Text size", type: "number", default: size, min: 10, max: 96 },
    { name: "displayUnits", label: "Display units", type: "select", default: "auto", options: [{ value: "auto", label: "Auto" }, { value: "none", label: "None" }, { value: "K", label: "Thousands" }, { value: "M", label: "Millions" }, { value: "B", label: "Billions" }] },
  ],
});

const noTitle = (b: Blocks): ObjectDef => ({ ...b.title, props: b.title.props.map((p) => (p.name === "show" ? { ...p, default: false } : p)) }) as ObjectDef;

export function extendRegistry(visuals: VisualDef[], keywords: Record<string, string[]>, b: Blocks): void {
  const cartesianObjects = (cat: string, val: string) => [b.title, b.legend, b.axis("categoryAxis", cat), b.axis("valueAxis", val), b.dataLabels, b.colors, b.background];
  const after = (type: string, ...defs: VisualDef[]) => {
    const i = visuals.findIndex((v) => v.type === type);
    visuals.splice(i < 0 ? visuals.length : i + 1, 0, ...defs);
  };
  after(
    "stackedBarChart",
    {
      type: "hundredPercentStackedColumnChart",
      label: "100% stacked column chart",
      icon: Columns4,
      roles: b.cartesianRoles(msg("X-axis")),
      objects: cartesianObjects(msg("X-axis"), msg("Y-axis")),
      size: { width: 480, height: 300 },
    },
    {
      type: "hundredPercentStackedBarChart",
      label: "100% stacked bar chart",
      icon: Rows4,
      roles: b.cartesianRoles(msg("Y-axis")),
      objects: cartesianObjects(msg("Y-axis"), msg("X-axis")),
      size: { width: 480, height: 300 },
    },
  );
  const comboRoles: RoleDef[] = [
    { name: "category", label: "X-axis", kind: "grouping", autoMax: 1 },
    { name: "values", label: "Column y-axis", kind: "value", autoMax: 1 },
    { name: "lineValues", label: "Line y-axis", kind: "value" },
    { name: "series", label: "Column legend", kind: "grouping", max: 1 },
  ];
  const comboObjects: ObjectDef[] = [
    b.title,
    b.legend,
    b.axis("categoryAxis", msg("X-axis")),
    b.axis("valueAxis", msg("Y-axis")),
    {
      name: "secondaryAxis",
      label: "Secondary y-axis",
      toggle: "show",
      props: [
        { name: "show", label: "Show", type: "bool", default: true },
        { name: "title", label: "Show title", type: "bool", default: false },
      ],
    },
    {
      name: "lines",
      label: "Lines",
      props: [
        { name: "width", label: "Width", type: "number", default: 2, min: 1, max: 8 },
        { name: "markers", label: "Markers", type: "bool", default: true },
        { name: "smooth", label: "Smooth", type: "bool", default: false },
      ],
    },
    b.dataLabels,
    b.colors,
    b.background,
  ];
  after(
    "lineChart",
    { type: "lineClusteredColumnComboChart", label: "Line and clustered column chart", icon: ChartNoAxesCombined, roles: comboRoles, objects: comboObjects, size: { width: 560, height: 320 } },
    { type: "lineStackedColumnComboChart", label: "Line and stacked column chart", icon: ChartColumnIncreasing, roles: comboRoles, objects: comboObjects, size: { width: 560, height: 320 } },
  );
  after(
    "scatterChart",
    {
      type: "waterfallChart",
      label: "Waterfall chart",
      icon: ChartCandlestick,
      roles: [
        { name: "category", label: "Category", kind: "grouping", autoMax: 1 },
        { name: "breakdown", label: "Breakdown", kind: "grouping", max: 1 },
        { name: "values", label: "Y-axis", kind: "value", max: 1 },
      ],
      objects: [
        b.title,
        b.legend,
        b.axis("categoryAxis", msg("X-axis")),
        b.axis("valueAxis", msg("Y-axis")),
        {
          name: "sentimentColors",
          label: "Sentiment colors",
          props: [
            { name: "increase", label: "Increase", type: "color", default: "" },
            { name: "decrease", label: "Decrease", type: "color", default: "" },
            { name: "total", label: "Total", type: "color", default: "" },
          ],
        },
        {
          name: "breakdown",
          label: "Breakdown",
          props: [
            { name: "max", label: "Max breakdowns", type: "number", default: 5, min: 1, max: 20 },
            { name: "total", label: "Show total", type: "bool", default: true },
          ],
        },
        b.dataLabels,
        b.background,
      ],
      size: { width: 520, height: 300 },
    },
    {
      type: "funnel",
      label: "Funnel",
      icon: Funnel,
      roles: [
        { name: "category", label: "Category", kind: "grouping", autoMax: 1 },
        { name: "values", label: "Values", kind: "value", max: 1 },
      ],
      objects: [
        b.title,
        {
          name: "dataLabels",
          label: "Data labels",
          toggle: "show",
          props: [
            { name: "show", label: "Show", type: "bool", default: true },
            { name: "percent", label: "Percent of first", type: "bool", default: true },
          ],
        },
        b.colors,
        b.background,
      ],
      size: { width: 420, height: 300 },
    },
    {
      type: "treemap",
      label: "Treemap",
      icon: LayoutDashboard,
      roles: [
        { name: "group", label: "Category", kind: "grouping", autoMax: 1 },
        { name: "details", label: "Details", kind: "grouping", max: 1 },
        { name: "values", label: "Values", kind: "value", max: 1 },
      ],
      objects: [
        b.title,
        {
          name: "dataLabels",
          label: "Data labels",
          toggle: "show",
          props: [
            { name: "show", label: "Show", type: "bool", default: true },
            { name: "values", label: "Show values", type: "bool", default: true },
          ],
        },
        b.background,
      ],
      size: { width: 480, height: 320 },
    },
    {
      type: "gauge",
      label: "Gauge",
      icon: Gauge,
      roles: [
        { name: "values", label: "Value", kind: "value", max: 1 },
        { name: "minValue", label: "Minimum value", kind: "value", max: 1 },
        { name: "maxValue", label: "Maximum value", kind: "value", max: 1 },
        { name: "targetValue", label: "Target value", kind: "value", max: 1 },
      ],
      objects: [
        b.title,
        {
          name: "gaugeAxis",
          label: "Gauge axis",
          props: [
            { name: "min", label: "Min", type: "text", default: "", placeholder: msg("Automatic") },
            { name: "max", label: "Max", type: "text", default: "", placeholder: msg("Automatic") },
            { name: "target", label: "Target", type: "text", default: "", placeholder: msg("None") },
          ],
        },
        { name: "colors", label: "Colors", props: [{ name: "fill", label: "Fill", type: "color", default: "" }, { name: "target", label: "Target", type: "color", default: "" }] },
        callout(28),
        b.background,
      ],
      size: { width: 320, height: 240 },
    },
    {
      type: "kpi",
      label: "KPI",
      icon: TrendingUp,
      roles: [
        { name: "indicator", label: "Value", kind: "value", max: 1 },
        { name: "trend", label: "Trend axis", kind: "grouping", max: 1 },
        { name: "goal", label: "Target", kind: "value", max: 1 },
      ],
      objects: [
        callout(36),
        {
          name: "trendline",
          label: "Trend axis",
          toggle: "show",
          props: [{ name: "show", label: "Show", type: "bool", default: true }],
        },
        {
          name: "goals",
          label: "Target label",
          toggle: "show",
          props: [
            { name: "show", label: "Show", type: "bool", default: true },
            { name: "distance", label: "Distance", type: "select", default: "percent", options: [{ value: "percent", label: "Percent" }, { value: "value", label: "Value" }, { value: "both", label: "Value and percent" }] },
          ],
        },
        {
          name: "status",
          label: "Color coding",
          props: [
            { name: "direction", label: "Direction", type: "select", default: "high", options: [{ value: "high", label: "High is good" }, { value: "low", label: "Low is good" }] },
          ],
        },
        noTitle(b),
        b.background,
      ],
      size: { width: 280, height: 200 },
    },
  );
  after("card", {
    type: "multiRowCard",
    label: "Multi-row card",
    icon: Rows3,
    roles: [{ name: "values", label: "Fields", kind: "value" }],
    objects: [
      callout(24),
      {
        name: "layout",
        label: "Layout",
        props: [
          { name: "columns", label: "Columns", type: "number", default: 0, min: 0, max: 8 },
          { name: "accent", label: "Accent bar", type: "bool", default: true },
        ],
      },
      { name: "label", label: "Category label", toggle: "show", props: [{ name: "show", label: "Show", type: "bool", default: true }] },
      noTitle(b),
      b.background,
    ],
    size: { width: 420, height: 140 },
  });

  for (const v of visuals) {
    if (MULTIPLES.includes(v.type)) {
      v.roles = [...v.roles, smallMultiplesRole];
      v.objects = [...v.objects.slice(0, -1), smallMultiples, ...v.objects.slice(-1)];
    }
    if (ANALYTICS.includes(v.type)) v.objects = [...v.objects.slice(0, -1), analytics, ...v.objects.slice(-1)];
    if (TOOLTIP_PAGES.includes(v.type)) v.objects = [...v.objects.slice(0, -1), tooltipObject, ...v.objects.slice(-1)];
    if (v.type === "table" || v.type === "matrix") v.objects = [...v.objects.slice(0, -1), conditionalFormatting, ...v.objects.slice(-1)];
  }

  Object.assign(keywords, {
    hundredPercentStackedColumnChart: ["100%", "percent", "share", "stacked", "column", "normalized"],
    hundredPercentStackedBarChart: ["100%", "percent", "share", "stacked", "bar", "normalized"],
    lineClusteredColumnComboChart: ["combo", "line", "column", "dual axis", "secondary axis"],
    lineStackedColumnComboChart: ["combo", "line", "stacked", "column", "dual axis"],
    waterfallChart: ["bridge", "variance", "increase", "decrease", "cascade"],
    funnel: ["pipeline", "conversion", "stages"],
    treemap: ["hierarchy", "rectangles", "share", "nested"],
    gauge: ["dial", "speedometer", "target", "progress", "radial"],
    kpi: ["indicator", "goal", "target", "trend", "status"],
    multiRowCard: ["cards", "new card", "several values", "kpi", "numbers", "callouts"],
  });
}

/** Charts that can show a report page as their tooltip. */
export const TOOLTIP_PAGES = [
  "columnChart",
  "barChart",
  "lineChart",
  "stackedColumnChart",
  "stackedBarChart",
  "hundredPercentStackedColumnChart",
  "hundredPercentStackedBarChart",
  "lineClusteredColumnComboChart",
  "lineStackedColumnComboChart",
  "pieChart",
  "donutChart",
];

export const conditionalFormatting: ObjectDef = {
  name: "conditionalFormatting",
  label: "Cell elements",
  editor: "conditionalFormatting",
  props: [],
};
