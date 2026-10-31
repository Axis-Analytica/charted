import { describe, expect, it } from "vitest";
import { prop, visualDef } from "./registry";
import { setStyleDefaults, styleDefaults, textTokens, themeDefault } from "./themeStyles";

const theme = {
  name: "Styled",
  dataColors: ["#123456"],
  textClasses: { title: { fontSize: 18, color: "#AA0000", fontFace: "Georgia" }, label: { fontSize: 10 }, callout: { fontSize: 40 } },
  visualStyles: {
    "*": {
      "*": {
        title: [{ show: false, alignment: "Center" }],
        legend: [{ show: true, position: "BottomCenter" }],
        labels: [{ show: true }],
        valueAxis: [{ gridlineShow: false, showAxisTitle: true }],
        border: [{ show: false }],
        dataPoint: [{ defaultColor: { solid: { color: "#00FF00" } } }],
        // Not something charted formats: kept in the theme, ignored here.
        zoom: [{ show: true }],
      },
    },
    clusteredColumnChart: { "*": { title: [{ show: true, fontSize: 20 }] } },
    card: { "*": { labels: [{ fontSize: 50 }], categoryLabels: [{ show: false }] } },
    pieChart: { "*": { title: [{ fontSize: 99 }] } },
    lineChart: { "*": { lineStyles: [{ strokeWidth: 4, showMarker: true }], legend: [{ position: "LeftCenter" }] } },
  },
};

describe("a theme's visual styles as format defaults", () => {
  const d = styleDefaults(theme);

  it("maps Power BI objects and properties to charted's", () => {
    expect(themeDefault("lineChart", "title", "show", d)).toBe(false);
    expect(themeDefault("lineChart", "title", "align", d)).toBe("center");
    expect(themeDefault("lineChart", "legend", "position", d)).toBe("bottom");
    expect(themeDefault("lineChart", "dataLabels", "show", d)).toBe(true);
    expect(themeDefault("lineChart", "valueAxis", "gridlines", d)).toBe(false);
    expect(themeDefault("lineChart", "valueAxis", "title", d)).toBe(true);
    expect(themeDefault("lineChart", "background", "border", d)).toBe(false);
    expect(themeDefault("lineChart", "colors", "fill", d)).toBe("#00FF00");
    expect(themeDefault("lineChart", "lines", "width", d)).toBe(4);
    expect(themeDefault("lineChart", "lines", "markers", d)).toBe(true);
    expect(themeDefault("lineChart", "zoom", "show", d)).toBeUndefined();
  });

  it("lets a visual type's styles override the wildcard, and those override text classes", () => {
    // clusteredColumnChart is charted's columnChart.
    expect(themeDefault("columnChart", "title", "show", d)).toBe(true);
    expect(themeDefault("columnChart", "title", "fontSize", d)).toBe(20);
    expect(themeDefault("barChart", "title", "fontSize", d)).toBe(18);
    expect(themeDefault("lineChart", "dataLabels", "fontSize", d)).toBe(10);
    expect(themeDefault("card", "callout", "fontSize", d)).toBe(50);
    expect(themeDefault("multiRowCard", "callout", "fontSize", d)).toBe(40);
    expect(themeDefault("card", "label", "show", d)).toBe(false);
  });

  it("comes between a visual's own formatting and the registry's defaults", () => {
    const def = visualDef("lineChart");
    setStyleDefaults(d);
    try {
      expect(prop(undefined, def, "title", "show")).toBe(false);
      expect(prop({ title: { show: true } }, def, "title", "show")).toBe(true);
      // A position charted doesn't have (left) leaves the wildcard's.
      expect(prop(undefined, def, "legend", "position")).toBe("bottom");
      // A value out of range falls back to the registry's default.
      expect(prop(undefined, visualDef("pieChart"), "title", "fontSize")).toBe(13);
      expect(prop(undefined, def, "categoryAxis", "show")).toBe(true);
    } finally {
      setStyleDefaults({});
    }
    expect(prop(undefined, def, "title", "show")).toBe(true);
  });

  it("styles titles and the page font from text classes", () => {
    expect(textTokens(theme)).toEqual({ "--report-title-fg": "#AA0000", "--report-title-font": "Georgia, \"Merriweather Variable\", var(--font-ui, sans-serif)" });
    expect(styleDefaults(null)).toEqual({});
  });
});
