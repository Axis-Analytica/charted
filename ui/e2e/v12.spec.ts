import { expect, type Locator, type Page, test } from "@playwright/test";

// v1.2: the new visual types, Top N filters at every level, conditional formatting,
// analytics lines, small multiples and report-page tooltips, built from the sample.

const shots = process.env.CHARTED_SHOTS;

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  if (await explore.isVisible()) await explore.click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
}

async function newPage(page: Page, name: string) {
  const tabs = page.locator(".page-tab");
  const before = await tabs.count();
  await page.getByRole("button", { name: "New page" }).click();
  await expect(tabs).toHaveCount(before + 1);
  const tab = tabs.last().getByRole("tab");
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await tab.dblclick();
  await page.getByRole("textbox", { name: "Page name" }).fill(name);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("tab", { name })).toHaveAttribute("aria-selected", "true");
}

async function shot(page: Page, name: string) {
  if (!shots) return;
  // Let charts finish their entry animation.
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${shots}/${name}.png` });
}

const fieldsPane = (page: Page) => page.locator(".fields-pane");
const visuals = (page: Page) => page.getByTestId("visual");

async function expandTable(page: Page, table: string) {
  const btn = fieldsPane(page).locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) });
  if ((await btn.locator("xpath=ancestor::li[1]").getAttribute("aria-expanded")) !== "true") await btn.click();
}

function fieldRow(page: Page, table: string, field: string) {
  const tableItem = fieldsPane(page).locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) }) });
  return tableItem.locator(".field-row", { has: page.locator(".field-name", { hasText: new RegExp(`^${field}$`) }) });
}

async function tick(page: Page, table: string, field: string) {
  await expandTable(page, table);
  await fieldRow(page, table, field).locator("input").click();
}

/** Adds a visual centred on `at` (page coordinates of a page `width` wide). */
async function addVisual(page: Page, label: string, at: { x: number; y: number }, width = 1280): Promise<Locator> {
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  const scale = pageBox.width / width;
  await page.mouse.click(pageBox.x + at.x * scale, pageBox.y + at.y * scale);
  const before = await visuals(page).count();
  await page.locator(".viz-gallery").getByRole("button", { name: label, exact: true }).click();
  await expect(visuals(page)).toHaveCount(before + 1);
  return visuals(page).nth(before);
}

async function removeVisual(page: Page, visual: Locator) {
  const before = await visuals(page).count();
  await visual.hover();
  await visual.getByRole("button", { name: "More options" }).click();
  await visual.getByRole("menuitem", { name: "Remove" }).click();
  await expect(visuals(page)).toHaveCount(before - 1);
}

async function dropInWell(page: Page, table: string, field: string, role: string) {
  await expandTable(page, table);
  await fieldRow(page, table, field).dragTo(page.locator(`.well[data-role="${role}"] .well-empty`));
  await expect(page.locator(`.well[data-role="${role}"] .chip-name`, { hasText: field }).first()).toBeVisible();
}

type Item = { value: number | null } | number | null | number[];
type ChartOption = {
  xAxis: { data?: string[]; type?: string }[];
  yAxis: { data?: string[]; type?: string }[];
  grid: unknown[];
  series: { id?: string; type?: string; yAxisIndex?: number; data: Item[]; markLine?: { data: ({ type?: string; name?: string } | { name?: string }[])[] } }[];
};
const option = (visual: Locator) => visual.locator(".cartesian").first().evaluate((el) => (el as unknown as { chart: { getOption(): unknown } }).chart.getOption() as ChartOption);
const num = (d: Item | undefined) => (d && typeof d === "object" && !Array.isArray(d) ? d.value : (d as number | null));
const values = async (visual: Locator, series: number) => ((await option(visual)).series[series]?.data ?? []).map(num);
const categories = async (visual: Locator) => {
  const o = await option(visual);
  return o.xAxis[0]?.data ?? o.yAxis[0]?.data ?? [];
};

/** A bar's centre on screen, through the chart's own geometry, once drawn. */
async function barAt(visual: Locator, index: number): Promise<{ x: number; y: number }> {
  const chart = visual.locator(".cartesian");
  const probe = () =>
    chart.evaluate((el, i) => {
      type Chart = {
        getOption(): { series: { data: { value: number }[] }[]; yAxis: { type: string }[] };
        convertToPixel(f: object, v: number[]): number[];
        getZr(): { handler: { findHover(x: number, y: number): { target?: unknown } } };
      };
      const c = (el as unknown as { chart: Chart }).chart;
      const o = c.getOption();
      const v = o.series[0]!.data[i]!.value;
      const zoom = el.getBoundingClientRect().width / (el as HTMLElement).offsetWidth;
      const point = o.yAxis[0]?.type === "category" ? [v / 2, i] : [i, v / 2];
      const px = c.convertToPixel({ seriesIndex: 0 }, point);
      return { at: px.map((x) => x * zoom), drawn: !!c.getZr().handler.findHover(px[0]!, px[1]!).target };
    }, index);
  await expect.poll(async () => (await probe()).drawn).toBe(true);
  const { at } = await probe();
  const box = (await chart.boundingBox())!;
  return { x: box.x + at[0]!, y: box.y + at[1]! };
}

async function formatSection(page: Page, name: string) {
  await page.getByRole("tab", { name: "Format" }).click();
  const section = page.locator(".format-section", { has: page.locator(".format-title", { hasText: new RegExp(`^${name}$`) }) });
  const title = section.locator(".format-title");
  if ((await title.getAttribute("aria-expanded")) !== "true") await title.click();
  return section;
}

test.setTimeout(300_000);
test.use({ actionTimeout: 15_000 });

test("new chart types: 100% stacked, combo, waterfall, funnel, treemap, gauge, KPI and the multi-row card", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Charts");

  // 100% stacked column: each category's channels add up to 100%, tooltips show both.
  const pct = await addVisual(page, "100% stacked column chart", { x: 300, y: 200 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await dropInWell(page, "Store", "Channel", "series");
  await expect.poll(async () => (await option(pct)).series.length).toBeGreaterThan(1);
  const o = await option(pct);
  for (let i = 0; i < (o.xAxis[0]?.data?.length ?? 0); i++) expect(o.series.reduce((s, x) => s + (num(x.data[i]) ?? 0), 0)).toBeCloseTo(1, 6);
  const tip = await pct.locator(".cartesian").evaluate((el) => {
    const c = (el as unknown as { chart: { getOption(): { tooltip: { formatter(p: unknown): string }[] } } }).chart;
    return c.getOption().tooltip[0]!.formatter([{ dataIndex: 0, seriesId: "v:0:0", marker: "" }]);
  });
  expect(tip).toMatch(/\$[\d,]+/);
  expect(tip).toMatch(/\d%\)/);
  await shot(page, "v12-01-100-percent");
  await removeVisual(page, pct);

  // Line and clustered column: Units as a line on its own axis, not split by the legend.
  const combo = await addVisual(page, "Line and clustered column chart", { x: 300, y: 200 });
  await tick(page, "Date", "Year");
  await tick(page, "Sales", "Total Sales");
  await tick(page, "Sales", "Units");
  await expect(page.locator('.well[data-role="lineValues"] .chip-name', { hasText: "Units" })).toBeVisible();
  await expect.poll(async () => (await option(combo)).series.map((s) => s.type).join()).toBe("bar,line");
  const comboOpt = await option(combo);
  expect(comboOpt.yAxis.length).toBe(2);
  expect(comboOpt.series[1]!.yAxisIndex).toBe(1);
  const units = await values(combo, 1);
  await dropInWell(page, "Store", "Channel", "series");
  await expect.poll(async () => (await option(combo)).series.filter((s) => s.type === "bar").length).toBeGreaterThan(1);
  const split = await option(combo);
  expect(split.series.find((s) => s.type === "line")!.data.map(num)).toEqual(units);
  await shot(page, "v12-02-combo");
  // Converting keeps the line: to line and stacked column.
  await page.locator(".viz-gallery").getByRole("button", { name: "Line and stacked column chart", exact: true }).click();
  await expect(combo).toHaveAttribute("data-type", "lineStackedColumnComboChart");
  await expect.poll(async () => (await option(combo)).series.some((s) => s.type === "line")).toBe(true);
  await removeVisual(page, combo);

  // Waterfall: years step up from the start, ending in the total.
  const fall = await addVisual(page, "Waterfall chart", { x: 300, y: 200 });
  await tick(page, "Date", "Year");
  await tick(page, "Sales", "Total Sales");
  await expect.poll(async () => (await option(fall)).series[0]?.data.length ?? 0).toBe(5);
  const steps = (await option(fall)).series[0]!.data as number[][];
  const deltas = steps.slice(0, 4).reduce((s, d) => s + (d[2]! - d[1]!), 0);
  expect(steps[4]![2]).toBeCloseTo(deltas, 2);
  expect(steps[4]![3]).toBe(2);
  // With a breakdown, each year's change by channel.
  await dropInWell(page, "Store", "Channel", "breakdown");
  await expect.poll(async () => (await categories(fall)).join()).toMatch(/Online|Store/);
  expect((await categories(fall)).filter((c) => /^\d{4}$/.test(c))).toEqual(["2022", "2023", "2024", "2025"]);
  await shot(page, "v12-03-waterfall");
  await removeVisual(page, fall);

  // Funnel and treemap: one stage or rectangle per category.
  const funnel = await addVisual(page, "Funnel", { x: 250, y: 200 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect.poll(async () => (await option(funnel)).series[0]?.data.length ?? 0).toBe(6);
  const stages = await values(funnel, 0);
  expect([...stages].sort((a, b) => (b ?? 0) - (a ?? 0))).toEqual(stages);
  const tree = await addVisual(page, "Treemap", { x: 800, y: 200 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect.poll(async () => (await option(tree)).series[0]?.data.length ?? 0).toBe(6);
  await shot(page, "v12-04-funnel-treemap");
  await removeVisual(page, tree);
  await removeVisual(page, funnel);

  // Gauge: the value against the target.
  const gauge = await addVisual(page, "Gauge", { x: 250, y: 200 });
  await tick(page, "Sales", "Total Sales");
  await dropInWell(page, "Targets", "Target", "targetValue");
  await expect.poll(async () => (await option(gauge)).series.length).toBe(2);
  const g = (await option(gauge)).series as unknown as { data: { value: number }[]; max: number; min: number }[];
  expect(g[0]!.data[0]!.value).toBeGreaterThan(0);
  expect(g[1]!.data[0]!.value).toBeGreaterThan(0);
  expect(g[0]!.max).toBeGreaterThan(g[0]!.data[0]!.value);

  // KPI: this year's sales against the target, with the trend behind.
  const kpi = await addVisual(page, "KPI", { x: 700, y: 200 });
  await tick(page, "Sales", "Total Sales");
  await tick(page, "Date", "Year");
  await dropInWell(page, "Targets", "Target", "goal");
  await expect(kpi.locator(".kpi-value")).toHaveText(/\$/);
  await expect(kpi.locator(".kpi-goal")).toContainText("Target:");
  await expect.poll(async () => (await option(kpi)).series[0]?.data.length ?? 0).toBe(4);
  await expect(kpi.locator(".kpi-visual")).toHaveClass(/is-(good|bad)/);

  // Multi-row card: a callout per value.
  const multi = await addVisual(page, "Multi-row card", { x: 1050, y: 500 });
  await tick(page, "Sales", "Total Sales");
  await tick(page, "Sales", "Units");
  await tick(page, "Sales", "Orders");
  await expect(multi.locator(".multi-card-item")).toHaveCount(3);
  await expect(multi.locator(".multi-card-item").first().locator(".card-value")).toHaveText(/\$/);
  await expect(multi.locator(".multi-card-item").nth(2).locator(".card-label")).toHaveText("Orders");
  await shot(page, "v12-05-gauge-kpi-cards");
});

test("Top N on a page and conditional formatting in a table", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Top three");

  // A page-level Top 3 categories by sales filters every visual on the page.
  const col = await addVisual(page, "Clustered column chart", { x: 300, y: 200 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect.poll(async () => (await categories(col)).length).toBe(6);
  const top3 = (await categories(col)).slice(0, 3);
  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await expandTable(page, "Product");
  const pageFilters = page.getByRole("group", { name: "Filters on this page" });
  await fieldRow(page, "Product", "Category").dragTo(pageFilters.locator(".filter-drop"));
  const card = pageFilters.locator(".filter-card", { hasText: "Category" });
  await card.locator(".filter-card-toggle").click();
  await card.getByRole("combobox", { name: "Filter type" }).selectOption("topN");
  await card.getByRole("spinbutton", { name: "Count" }).fill("3");
  await card.getByRole("combobox", { name: "By value" }).selectOption({ label: "Total Sales" });
  await card.getByRole("button", { name: "Apply filter" }).click();
  await expect(card.locator(".filter-card-summary")).toHaveText("Top 3");
  await expect.poll(async () => (await categories(col)).join()).toBe(top3.join());
  await shot(page, "v12-06-top-n");

  // Conditional formatting: a background gradient and data bars on Total Sales, and
  // icons based on Units (a measure not in the table).
  const table = await addVisual(page, "Table", { x: 900, y: 300 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect(table.locator("tbody tr")).toHaveCount(3);
  const cells = await formatSection(page, "Cell elements");
  const kinds = cells.locator(".cf-column", { hasText: "Total Sales" });
  await kinds.getByRole("button", { name: "Background color" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("Background color");
  await dialog.getByRole("button", { name: "OK" }).click();
  await expect(kinds.getByRole("button", { name: "Background color" })).toHaveAttribute("aria-pressed", "true");
  const bg = (r: number) => table.locator("tbody tr").nth(r).locator("td").nth(1).evaluate((el) => getComputedStyle(el).backgroundColor);
  await expect.poll(() => bg(0)).not.toBe("rgba(0, 0, 0, 0)");
  expect(await bg(0)).not.toBe(await bg(2));
  await kinds.getByRole("button", { name: "Data bars" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "OK" }).click();
  await expect(table.locator("[data-cf-bar]")).toHaveCount(3);
  const widths = await table.locator("[data-cf-bar]").evaluateAll((els) => els.map((e) => parseFloat((e as HTMLElement).style.width)));
  expect(widths[0]).toBeGreaterThan(widths[2]!);
  await kinds.getByRole("button", { name: "Icons" }).click();
  await page.getByRole("dialog").getByRole("combobox", { name: "What field should we base this on?" }).selectOption({ label: "Sales[Units]" });
  await page.getByRole("dialog").getByRole("button", { name: "OK" }).click();
  await expect(table.locator("[data-cf-icon]")).toHaveCount(3);
  await expect(table.locator('[data-cf-icon="up"]')).toHaveCount(1);
  await shot(page, "v12-07-conditional-formatting");
});

test("analytics lines, small multiples and a report-page tooltip", async ({ page }) => {
  await openSample(page);

  // A tooltip page: small, with a card, used for points with a Category.
  await newPage(page, "Category tip");
  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await page.getByRole("tab", { name: "Format" }).click();
  await page.locator(".viz-pane").getByRole("checkbox", { name: "Use as a tooltip" }).click();
  await expect(page.locator(".page-tab.is-tooltip")).toHaveCount(1);
  await page.getByRole("tab", { name: "Build" }).click();
  const tipCard = await addVisual(page, "Card", { x: 160, y: 120 }, 320);
  await tick(page, "Sales", "Total Sales");
  await expect(tipCard.locator(".card-value")).toHaveText(/\$/);
  const whole = await tipCard.locator(".card-value").textContent();
  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await expandTable(page, "Product");
  await fieldRow(page, "Product", "Category").dragTo(page.getByRole("group", { name: "Tooltip fields" }).locator(".filter-drop"));
  await expect(page.getByRole("group", { name: "Tooltip fields" }).locator(".filter-card-name")).toHaveText("Category");

  await newPage(page, "Analytics");
  const col = await addVisual(page, "Clustered column chart", { x: 300, y: 200 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect.poll(async () => (await categories(col)).length).toBe(6);

  // Hovering a bar shows the tooltip page for that category.
  const at = await barAt(col, 0);
  await page.mouse.move(at.x, at.y);
  const popover = page.locator(".page-tooltip");
  await expect(popover).toBeVisible();
  await expect(popover.locator(".card-value")).toHaveText(/\$/);
  await expect(popover.locator(".card-value")).not.toHaveText(whole ?? "");
  await shot(page, "v12-08-tooltip-page");
  await page.mouse.move(5, 5);
  await expect(popover).toHaveCount(0);
  // The format pane can turn it off.
  const tips = await formatSection(page, "Tooltips");
  await tips.getByRole("combobox", { name: "Tooltip page" }).selectOption("default");
  await page.mouse.move(at.x, at.y);
  await page.waitForTimeout(300);
  await expect(popover).toHaveCount(0);

  // Analytics: average and trend lines.
  const analytics = await formatSection(page, "Analytics");
  await analytics.getByRole("checkbox", { name: "Average line" }).click();
  await analytics.getByRole("checkbox", { name: "Trend line" }).click();
  await expect.poll(async () => ((await option(col)).series[0]?.markLine?.data ?? []).length).toBe(2);
  const marks = (await option(col)).series[0]!.markLine!.data;
  expect((marks[0] as { type?: string }).type).toBe("average");
  expect(Array.isArray(marks[1])).toBe(true);
  await shot(page, "v12-09-analytics");
  await page.getByRole("tab", { name: "Build" }).click();

  // Small multiples: one small chart per region, sharing the axes.
  await dropInWell(page, "Store", "Region", "smallMultiples");
  await expect.poll(async () => (await option(col)).grid.length).toBeGreaterThan(1);
  const multiples = await option(col);
  expect(multiples.xAxis.length).toBe(multiples.grid.length);
  const maxes = (multiples.yAxis as unknown as { max?: number }[]).map((y) => y.max);
  expect(new Set(maxes).size).toBe(1);
  await shot(page, "v12-10-small-multiples");
});
