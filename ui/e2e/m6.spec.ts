import { readFileSync } from "node:fs";
import { expect, type Locator, type Page, test } from "@playwright/test";

const shots = process.env.CHARTED_SHOTS;

// Pane layout is kept by the core across tests: close the panes a test opened.
test.afterEach(async ({ page }) => {
  for (const name of ["Bookmarks pane", "Selection pane", "Sync slicers pane", "Agent pane"]) {
    const b = page.getByRole("button", { name, exact: true });
    if ((await b.count()) && (await b.first().getAttribute("aria-pressed")) === "true") await b.first().click();
  }
});

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
  if (shots) await page.screenshot({ path: `${shots}/${name}.png` });
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

async function addVisual(page: Page, label: string, at: { x: number; y: number }): Promise<Locator> {
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  const scale = pageBox.width / 1280;
  await page.mouse.click(pageBox.x + at.x * scale, pageBox.y + at.y * scale);
  const before = await visuals(page).count();
  await page.locator(".viz-gallery").getByRole("button", { name: label, exact: true }).click();
  await expect(visuals(page)).toHaveCount(before + 1);
  return visuals(page).nth(before);
}

async function dropInWell(page: Page, table: string, field: string, role: string) {
  await expandTable(page, table);
  await fieldRow(page, table, field).dragTo(page.locator(`.well[data-role="${role}"] .well-empty`));
  await expect(page.locator(`.well[data-role="${role}"] .chip-name`, { hasText: field }).first()).toBeVisible();
}

async function newMeasure(page: Page, formula: string, name: string) {
  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await page.keyboard.press("Control+m");
  const editor = page.locator(".formula-bar .dax-editor");
  await expect(editor).toBeVisible();
  await editor.click();
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Delete");
  await page.keyboard.insertText(formula);
  await page.locator(".formula-bar").getByRole("button", { name: "Commit" }).click();
  await expandTable(page, "Sales");
  await expect(fieldsPane(page).locator(".field-name", { hasText: new RegExp(`^${name}$`) })).toHaveCount(1, { timeout: 15_000 });
}

type ChartOption = { xAxis: { data?: string[] }[]; yAxis: { data?: string[] }[]; series: { data: ({ value: number | null } | number | null)[] }[] };
const option = (visual: Locator) => visual.locator(".cartesian").evaluate((el) => ((el as unknown as { chart?: { getOption(): unknown } }).chart?.getOption() ?? { xAxis: [], yAxis: [], grid: [], series: [] }) as ChartOption);
const categories = async (visual: Locator) => {
  const o = await option(visual);
  return o.xAxis[0]?.data ?? o.yAxis[0]?.data ?? [];
};
const values = async (visual: Locator, series: number) => ((await option(visual)).series[series]?.data ?? []).map((d) => (d && typeof d === "object" ? d.value : d));

/** A bar's centre on screen, through the chart's own geometry, once the bar is drawn
 * there (charts animate in when motion is on, as on CI). */
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
      // Bar charts put the categories on the y axis.
      const point = o.yAxis[0]?.type === "category" ? [v / 2, i] : [i, v / 2];
      const px = c.convertToPixel({ seriesIndex: 0 }, point);
      return { at: px.map((x) => x * zoom), drawn: !!c.getZr().handler.findHover(px[0]!, px[1]!).target };
    }, index);
  await expect.poll(async () => (await probe()).drawn).toBe(true);
  const { at } = await probe();
  const box = (await chart.boundingBox())!;
  return { x: box.x + at[0]!, y: box.y + at[1]! };
}

test.setTimeout(240_000);
test.use({ actionTimeout: 15_000 });

test("a Trends page: last year and year to date, drill down and up, drill through, bookmarks", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Trends");

  // Time intelligence: last year's sales next to this year's, by month.
  await newMeasure(page, "Sales LY = CALCULATE ( [Total Sales], SAMEPERIODLASTYEAR ( 'Date'[Date] ) )", "Sales LY");
  await newMeasure(page, "Sales YTD = TOTALYTD ( [Total Sales], 'Date'[Date] )", "Sales YTD");
  const line = await addVisual(page, "Line chart", { x: 330, y: 170 });
  await tick(page, "Date", "YearMonth");
  await tick(page, "Sales", "Total Sales");
  await tick(page, "Sales", "Sales LY");
  await expect.poll(async () => (await option(line)).series.length).toBe(2);
  // Last year's values arrive after the series appears.
  await expect.poll(async () => (await values(line, 1))[12] ?? null).not.toBeNull();
  const sales = await values(line, 0);
  const ly = await values(line, 1);
  expect(ly.slice(0, 12).every((v) => v === null)).toBe(true);
  expect(ly[12]).toBeCloseTo(sales[0]!, 2);
  expect(ly[30]).toBeCloseTo(sales[18]!, 2);

  // Year to date in a table: December's YTD is the year's total.
  const table = await addVisual(page, "Table", { x: 330, y: 540 });
  await tick(page, "Date", "Year");
  await tick(page, "Sales", "Total Sales");
  await tick(page, "Sales", "Sales YTD");
  await expect(table.locator("tbody tr")).toHaveCount(4);
  const row = table.locator("tbody tr").nth(2);
  const num = async (cell: Locator) => Number(((await cell.textContent()) ?? "").replace(/[$,]/g, ""));
  await expect.poll(() => num(row.locator("td").nth(2))).toBeCloseTo(await num(row.locator("td").nth(1)), 0);

  // A column chart over a Year → Quarter → Month hierarchy.
  const column = await addVisual(page, "Clustered column chart", { x: 980, y: 170 });
  await dropInWell(page, "Date", "Year", "category");
  await dropInWell(page, "Date", "Quarter", "category");
  await dropInWell(page, "Date", "MonthName", "category");
  await tick(page, "Sales", "Total Sales");
  await expect.poll(() => categories(column)).toEqual(["2022", "2023", "2024", "2025"]);
  await shot(page, "m6-01-trends");

  // Drill mode: clicking 2024 shows its quarters; drill up; then expand all down.
  await column.hover();
  await column.getByRole("button", { name: "Drill down mode" }).click();
  await expect(column.getByRole("button", { name: "Drill down mode" })).toHaveAttribute("aria-pressed", "true");
  const y2024 = await barAt(column, 2);
  await page.mouse.click(y2024.x, y2024.y);
  await expect.poll(async () => (await categories(column)).sort()).toEqual(["Q1", "Q2", "Q3", "Q4"]);
  await shot(page, "m6-02-drilled");
  // Further: the months of the first quarter shown.
  const q = await barAt(column, 0);
  await page.mouse.click(q.x, q.y);
  await expect.poll(async () => (await categories(column)).length).toBe(3);
  await column.getByRole("button", { name: "Drill up" }).click();
  await column.getByRole("button", { name: "Drill up" }).click();
  await expect.poll(() => categories(column)).toEqual(["2022", "2023", "2024", "2025"]);
  await column.getByRole("button", { name: "Drill down mode" }).click();
  await column.getByRole("button", { name: "Expand all down one level in the hierarchy" }).click();
  await expect.poll(async () => (await categories(column)).length).toBe(16);
  expect((await categories(column))[0]).toMatch(/^20\d\d Q\d$/);
  await column.getByRole("button", { name: "Drill up" }).click();
  await expect.poll(async () => (await categories(column)).length).toBe(4);

  // Drill through: a Category detail page, reached from a bar of categories.
  await newPage(page, "Category detail");
  const card = await addVisual(page, "Card", { x: 200, y: 100 });
  await tick(page, "Sales", "Total Sales");
  await expect(card.locator(".card-value")).toHaveText(/\$/);
  const whole = await card.locator(".card-value").textContent();
  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await expandTable(page, "Product");
  await fieldRow(page, "Product", "Category").dragTo(page.getByRole("group", { name: "Drill through" }).locator(".filter-drop"));
  await expect(page.getByRole("group", { name: "Drill through" }).locator(".drill-field-name")).toHaveText("Category");

  await page.getByRole("tab", { name: "Trends" }).click();
  const bars = await addVisual(page, "Clustered bar chart", { x: 980, y: 540 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect.poll(async () => (await categories(bars)).length).toBe(6);
  // The bars rank by value once the values arrive: wait until their order settles.
  let order = "";
  await expect
    .poll(async () => {
      const now = (await categories(bars)).join("|");
      const settled = now === order;
      order = now;
      return settled;
    }, { intervals: [500] })
    .toBe(true);
  const first = (await categories(bars))[0]!;
  const at = await barAt(bars, 0);
  await page.mouse.click(at.x, at.y, { button: "right" });
  const menu = page.getByRole("menu", { name: "Data point" });
  await expect(menu.getByRole("group", { name: "Drill through" })).toBeVisible();
  await menu.getByRole("menuitem", { name: "Category detail" }).click();
  await expect(page.getByRole("tab", { name: "Category detail" })).toHaveAttribute("aria-selected", "true");
  const bar = page.getByRole("region", { name: "Drill through" });
  await expect(bar).toContainText(`Category: ${first}`);
  await expect(card.locator(".card-value")).not.toHaveText(whole ?? "");
  await shot(page, "m6-03-drillthrough");
  await bar.getByRole("button", { name: "Back to Trends" }).click();
  await expect(page.getByRole("tab", { name: "Trends" })).toHaveAttribute("aria-selected", "true");

  // Bookmarks: the years, then 2024's quarters; step between them with the keyboard.
  await page.getByRole("button", { name: "Bookmarks pane" }).click();
  const pane = page.getByRole("region", { name: "Bookmarks" }).or(page.locator(".bookmarks-pane"));
  await pane.getByRole("button", { name: "Add bookmark" }).click();
  await expect(pane.locator(".bookmark-name")).toHaveText(["Bookmark 1"]);
  await column.hover();
  await column.getByRole("button", { name: "Drill down mode" }).click();
  const again = await barAt(column, 2);
  await page.mouse.click(again.x, again.y);
  await expect.poll(async () => (await categories(column)).length).toBe(4);
  await expect.poll(async () => (await categories(column))[0]).toMatch(/^Q/);
  await pane.getByRole("button", { name: "Add bookmark" }).click();
  await expect(pane.locator(".bookmark-name")).toHaveText(["Bookmark 1", "Bookmark 2"]);
  await pane.locator(".bookmark-name").nth(1).focus();
  await page.keyboard.press("F2");
  await page.getByRole("textbox", { name: "Bookmark name" }).fill("2024 by quarter");
  await page.keyboard.press("Enter");
  await expect(pane.locator(".bookmark-name").nth(1)).toHaveText("2024 by quarter");

  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await page.keyboard.press("Control+Alt+ArrowLeft");
  await expect.poll(() => categories(column)).toEqual(["2022", "2023", "2024", "2025"]);
  await expect(pane.locator(".bookmark.is-on")).toHaveText("Bookmark 1");
  await page.keyboard.press("Control+Alt+ArrowRight");
  await expect.poll(async () => (await categories(column))[0]).toMatch(/^Q/);
  await expect(pane.locator(".bookmark.is-on")).toHaveText("2024 by quarter");
  await shot(page, "m6-04-bookmarks");

  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.locator(".dirty-dot")).toHaveCount(0);
});

const pick = (page: Page, path: string) => page.evaluate((p) => ((window as unknown as { __chartedPick: () => string }).__chartedPick = () => p), path);
const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const contoso = new URL("../../crates/core/tests/fixtures/Contoso/Contoso.pbip", import.meta.url).pathname;

async function fileMenu(page: Page, item: string) {
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: item }).click();
}

test("import a Power BI project, see what didn't come across, export and import it again, share a bundle", async ({ page }) => {
  await openSample(page);
  await pick(page, contoso);
  await fileMenu(page, "Import Power BI project…");
  const report = page.getByRole("dialog", { name: "Imported Contoso.pbip" });
  await expect(report).toBeVisible({ timeout: 30_000 });
  await expect(report).toContainText("6 tables (2 with data)");
  for (const s of ["Regional managers", "Sql.Database", "map"]) await expect(report).toContainText(s);
  // The calculation group comes across.
  await expect(report).not.toContainText("Time Calculations");
  await shot(page, "m6-05-import-report");
  await report.getByRole("button", { name: "Done" }).click();
  await expect(report).toBeHidden();
  await expect(page.getByRole("tab", { name: "Overview" })).toHaveAttribute("aria-selected", "true");
  await expect(visuals(page)).toHaveCount(4);
  await expect(visuals(page).filter({ has: page.locator(".card-value") }).locator(".card-value")).toHaveText(/\$/);
  await shot(page, "m6-06-imported");
  // Its hierarchy came across (v1.1).
  await page.locator(".fields-pane").getByLabel("Search fields").fill("Category Hierarchy");
  await expect(page.locator(".fields-pane .field-row.is-hierarchy", { hasText: "Category Hierarchy" })).toHaveCount(1);
  await page.locator(".fields-pane").getByLabel("Search fields").fill("");

  // Export for Power BI, and import that: everything comes back.
  await pick(page, `${tmp}/export`);
  await fileMenu(page, "Export Power BI project…");
  const exporting = page.getByRole("dialog", { name: "Export Power BI project" });
  await expect(exporting.getByRole("checkbox", { name: "Copy the data files into the export" })).toBeChecked();
  await exporting.getByRole("textbox", { name: "Folder Power BI reads the data from" }).fill("C:\\Users\\analyst\\Contoso\\");
  await exporting.getByRole("button", { name: "Export" }).click();
  await expect(page.locator(".toast", { hasText: "Exported a Power BI project" })).toBeVisible();
  expect(readFileSync(`${tmp}/export/Contoso.SemanticModel/definition/expressions.tmdl`, "utf8")).toContain('expression SourceFolder = "C:\\Users\\analyst\\Contoso\\"');
  await pick(page, `${tmp}/export/Contoso.pbip`);
  await fileMenu(page, "Import Power BI project…");
  const again = page.getByRole("dialog", { name: "Imported Contoso.pbip" });
  await expect(again).toBeVisible({ timeout: 30_000 });
  // Only the role's dynamic filter is noted again (it comes in, but View as can't apply it).
  await expect(again).toContainText("Not imported, or only partly (1)");
  await expect(again).toContainText("Regional managers: Orders");
  await again.getByRole("button", { name: "Done" }).click();
  await expect(page.getByRole("tab", { name: "Trend" })).toBeVisible();

  // A bundle to share, opened from the welcome screen.
  await pick(page, `${tmp}/Shared.chartedz`);
  await fileMenu(page, "Save as bundle…");
  await expect(page.locator(".toast", { hasText: "Saved a bundle" })).toBeVisible();
  await fileMenu(page, "Close project");
  await pick(page, `${tmp}/Shared.chartedz`);
  await page.getByRole("button", { name: "Open a bundle or Power BI project…" }).click();
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible({ timeout: 30_000 });
  await expect(visuals(page)).toHaveCount(4);
});

test("a new project from scratch: name it, get data, build a visual", async ({ page }) => {
  await openSample(page);
  await fileMenu(page, "Close project");
  await page.getByRole("button", { name: "New project…" }).click();
  const dialog = page.getByRole("dialog", { name: "New project" });
  await dialog.getByRole("textbox", { name: "Project name" }).fill("Budget 2026");
  await expect(dialog).toContainText("charted/Budget 2026");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(page.locator(".topbar")).toContainText("Budget 2026");
  await expect(page.locator(".page-empty")).toContainText("This project has no data yet");
  await shot(page, "m6-07-new-project");
  await page.getByTestId("report-page").getByRole("button", { name: "Get data" }).click();
  await expect(page.locator(".qe")).toBeVisible();
  await pick(page, new URL("../../crates/core/tests/fixtures/Contoso/data/orders.csv", import.meta.url).pathname);
  await page.locator(".qe-ribbon").getByRole("button", { name: "New source" }).click();
  await page.getByRole("menuitem", { name: "Text/CSV" }).click();
  await expect(page.locator(".qe-query.is-on")).toContainText("orders");
  await page.getByRole("button", { name: "Close & Apply" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
  const card = await addVisual(page, "Card", { x: 200, y: 120 });
  await tick(page, "orders", "Amount");
  await expect(card.locator(".card-value")).toHaveText(/\d/);
  await page.keyboard.press("Control+s");
  await expect(page.locator(".dirty-dot")).toHaveCount(0);
});
