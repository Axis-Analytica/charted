// v1.1 modelling in the Fields pane: display folders, the properties dialog for measures
// and columns, hierarchies (created, filled and drilled in a column chart), and groups:
// a bin and a list group used in visuals. Everything it adds to the shared sample is
// removed again at the end.
import { expect, type Locator, type Page, test } from "@playwright/test";

const shots = process.env.CHARTED_SHOTS;

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  // Another spec may have left a different project open.
  if (!(await explore.isVisible()) && !(await page.locator(".topbar").textContent())?.includes("Harbor & Pine")) {
    await page.getByRole("button", { name: "File" }).click();
    await page.getByRole("menuitem", { name: "Close project" }).click();
    await expect(explore).toBeVisible();
  }
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
const tableItem = (page: Page, table: string) => fieldsPane(page).locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) }) });
const folderItem = (scope: Locator, name: string) => scope.locator("li.field-folder-item", { has: scope.page().locator(".field-folder", { hasText: new RegExp(`^${name}$`) }) }).first();

async function expandTable(page: Page, table: string) {
  const btn = fieldsPane(page).locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) });
  if ((await btn.locator("xpath=ancestor::li[1]").getAttribute("aria-expanded")) !== "true") await btn.click();
}

/** A field's row (not a hierarchy level). */
function fieldRow(page: Page, table: string, field: string) {
  return tableItem(page, table).locator(".field-row:not(.is-level)", { has: page.locator(".field-name", { hasText: new RegExp(`^${field.replace(/[()]/g, "\\$&")}$`) }) });
}

async function tick(page: Page, table: string, field: string) {
  await expandTable(page, table);
  await fieldRow(page, table, field).locator("input").click();
}

async function fieldMenu(page: Page, table: string, field: string, item: string) {
  await expandTable(page, table);
  await fieldRow(page, table, field).click({ button: "right" });
  await page.getByRole("menuitem", { name: item, exact: true }).click();
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

type ChartOption = { xAxis: { data?: string[] }[]; yAxis: { data?: string[] }[]; series: { data: ({ value: number | null } | number | null)[] }[] };
const option = (visual: Locator) => visual.locator(".cartesian").evaluate((el) => (el as unknown as { chart: { getOption(): unknown } }).chart.getOption() as ChartOption);
const categories = async (visual: Locator) => {
  const o = await option(visual);
  return o.xAxis[0]?.data ?? o.yAxis[0]?.data ?? [];
};

/** A bar's centre on screen, through the chart's own geometry, once it is drawn. */
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

const dialog = (page: Page) => page.getByRole("dialog");

test.setTimeout(240_000);
test.use({ actionTimeout: 15_000 });

test("display folders and the properties dialog for measures and columns", async ({ page }) => {
  await openSample(page);
  await expandTable(page, "Sales");
  const sales = tableItem(page, "Sales");

  // The sample's measures sit in a Sales folder, which collapses and opens.
  const folder = folderItem(sales, "Sales");
  await expect(folder).toHaveAttribute("aria-expanded", "true");
  await expect(folder.locator(".field-name", { hasText: /^Total Sales$/ })).toBeVisible();
  await folder.locator(".field-folder").first().click();
  await expect(folder).toHaveAttribute("aria-expanded", "false");
  await expect(fieldRow(page, "Sales", "Total Sales")).toHaveCount(0);
  await folder.locator(".field-folder").first().click();
  await expect(fieldRow(page, "Sales", "Total Sales")).toBeVisible();

  // Dragging a column onto the folder moves it in; onto the table, back out.
  await fieldRow(page, "Sales", "Quantity").dragTo(folder.locator(".field-folder").first());
  await expect(folder.locator(".field-name", { hasText: /^Quantity$/ })).toBeVisible();
  await fieldRow(page, "Sales", "Quantity").dragTo(sales.locator(".field-table").first());
  await expect(folder.locator(".field-name", { hasText: /^Quantity$/ })).toHaveCount(0);
  await expect(fieldRow(page, "Sales", "Quantity")).toBeVisible();

  // A measure's properties: description (the hover text), format and a nested folder.
  await fieldMenu(page, "Sales", "Total Sales", "Properties…");
  await expect(dialog(page)).toContainText("Measure properties");
  await dialog(page).getByLabel("Description").fill("Sales before returns");
  await dialog(page).getByLabel("Format", { exact: true }).selectOption({ label: "Decimal number" });
  await expect(dialog(page).getByLabel("Format string")).toHaveValue("#,0.00");
  await dialog(page).getByLabel("Display folder").fill("Money\\Revenue");
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(dialog(page)).toHaveCount(0);
  const money = folderItem(sales, "Money");
  const revenue = folderItem(money, "Revenue");
  await expect(revenue.locator(".field-name", { hasText: /^Total Sales$/ })).toBeVisible();
  await expect(fieldRow(page, "Sales", "Total Sales")).toHaveAttribute("title", "Sales before returns");
  // Search finds fields by their folder.
  await fieldsPane(page).getByLabel("Search fields").fill("revenue");
  await expect(fieldsPane(page).locator(".field-name")).toHaveText(["Total Sales"]);
  await fieldsPane(page).getByLabel("Search fields").fill("");
  await shot(page, "v11-01-folders");

  // Back as it was.
  await fieldMenu(page, "Sales", "Total Sales", "Properties…");
  await dialog(page).getByLabel("Description").fill("");
  await dialog(page).getByLabel("Format string").fill("\\$#,0");
  await dialog(page).getByLabel("Display folder").fill("Sales");
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(folderItem(sales, "Money")).toHaveCount(0);
  await expect(folder.locator(".field-name", { hasText: /^Total Sales$/ })).toBeVisible();

  // Moving a measure to another table keeps the visuals that show it working.
  await newPage(page, "Model props");
  await tick(page, "Targets", "Target");
  const card = visuals(page).last();
  await expect(card).toContainText(/\$[\d,]+/);
  const value = await card.textContent();
  await fieldMenu(page, "Targets", "Target", "Properties…");
  await dialog(page).getByLabel("Home table").selectOption({ label: "Date" });
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(fieldRow(page, "Targets", "Target")).toHaveCount(0);
  await expandTable(page, "Date");
  await expect(fieldRow(page, "Date", "Target")).toBeVisible();
  await expect(card).toHaveText(value!);
  await fieldMenu(page, "Date", "Target", "Properties…");
  await dialog(page).getByLabel("Home table").selectOption({ label: "Targets" });
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(fieldRow(page, "Targets", "Target")).toBeVisible();

  // Column properties: a sort-by column is checked against the data.
  await expandTable(page, "Product");
  await fieldMenu(page, "Product", "Category", "Properties…");
  await expect(dialog(page)).toContainText("Column properties");
  await dialog(page).getByLabel("Sort by column").selectOption({ label: "Subcategory" });
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(page.locator(".toast", { hasText: "can't be sorted by Subcategory" })).toBeVisible();
  await expect(dialog(page)).toBeVisible();
  await dialog(page).getByLabel("Sort by column").selectOption({ label: "(This column)" });
  await dialog(page).getByLabel("Description").fill("The product's category");
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(fieldRow(page, "Product", "Category")).toHaveAttribute("title", "The product's category");
});

test("a hierarchy drills in a column chart; a bin and a list group in visuals", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Hierarchies and groups");

  // Create a hierarchy from Year, then add Quarter and MonthName to it.
  await fieldMenu(page, "Date", "Year", "Create hierarchy");
  const rename = fieldsPane(page).getByRole("textbox", { name: "New name" });
  await expect(rename).toHaveValue("Year Hierarchy");
  await rename.fill("Calendar");
  await rename.press("Enter");
  await fieldMenu(page, "Date", "Quarter", "Add to hierarchy Calendar");
  await fieldMenu(page, "Date", "MonthName", "Add to hierarchy Calendar");
  const calendar = tableItem(page, "Date").locator("li.field-hierarchy", { has: page.locator(".field-name", { hasText: /^Calendar$/ }) });
  await expect(calendar.locator(".is-level .field-name")).toHaveText(["Year", "Quarter", "MonthName"]);

  // Dragged onto the axis, it brings its levels; drilling works as with separate fields.
  const column = await addVisual(page, "Clustered column chart", { x: 330, y: 170 });
  await calendar.locator(".field-row.is-hierarchy").dragTo(page.locator('.well[data-role="category"] .well-empty'));
  await expect(page.locator('.well[data-role="category"] .chip-name')).toHaveText(["Year", "Quarter", "MonthName"]);
  await tick(page, "Sales", "Total Sales");
  await expect.poll(() => categories(column)).toEqual(["2022", "2023", "2024", "2025"]);
  await column.hover();
  await column.getByRole("button", { name: "Drill down mode" }).click();
  const y2024 = await barAt(column, 2);
  await page.mouse.click(y2024.x, y2024.y);
  await expect.poll(async () => (await categories(column)).sort()).toEqual(["Q1", "Q2", "Q3", "Q4"]);
  await shot(page, "v11-02-hierarchy");
  await column.getByRole("button", { name: "Drill up" }).click();
  await column.getByRole("button", { name: "Drill down mode" }).click();
  await expect.poll(() => categories(column)).toEqual(["2022", "2023", "2024", "2025"]);

  // A bin of Amount, by size, on a column chart's axis.
  await fieldMenu(page, "Sales", "Amount", "New group…");
  await expect(dialog(page).getByLabel("Name", { exact: true })).toHaveValue("Amount (bins)");
  await dialog(page).getByLabel("Bin size").fill("500");
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(dialog(page)).toHaveCount(0);
  const bins = await addVisual(page, "Clustered column chart", { x: 980, y: 170 });
  await tick(page, "Sales", "Amount (bins)");
  await tick(page, "Sales", "Orders");
  await expect.poll(async () => (await categories(bins)).length).toBeGreaterThan(3);
  expect((await categories(bins)).every((c) => Number(c.replace(/[$,]/g, "")) % 500 === 0)).toBe(true);

  // A list group of categories, with everything else in Other, in a table.
  await page.getByTestId("report-page").click({ position: { x: 5, y: 5 } });
  await fieldMenu(page, "Product", "Category", "New group…");
  await expect(dialog(page).getByLabel("Name", { exact: true })).toHaveValue("Category (groups)");
  const ungrouped = dialog(page).getByRole("list", { name: "Ungrouped values" });
  await ungrouped.getByLabel("Camping").check();
  await ungrouped.getByLabel("Packs").check();
  await dialog(page).getByRole("button", { name: "Group", exact: true }).click();
  await expect(dialog(page).getByLabel("Group 1 name")).toHaveValue("Camping & Packs");
  await dialog(page).getByLabel("Group 1 name").fill("Outdoor");
  await expect(ungrouped.getByLabel("Camping")).toHaveCount(0);
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(dialog(page)).toHaveCount(0);
  await tick(page, "Product", "Category (groups)");
  const table = visuals(page).last();
  await tick(page, "Sales", "Total Sales");
  await expect(table.locator("tbody tr")).toHaveCount(2);
  await expect(table.locator("tbody tr td:first-child")).toHaveText(["Other", "Outdoor"]);
  await shot(page, "v11-03-groups");

  // Reopening the group shows its groups; renaming one updates the column.
  await fieldMenu(page, "Product", "Category (groups)", "Edit groups…");
  await expect(dialog(page).getByLabel("Group 1 name")).toHaveValue("Outdoor");
  await dialog(page).getByLabel("Other group name").fill("Everything else");
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(table.locator("tbody tr td:first-child")).toHaveText(["Everything else", "Outdoor"]);

  // Leave the shared sample as other specs expect it.
  await calendar.locator(".field-row.is-hierarchy").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await expect(calendar).toHaveCount(0);
  await fieldMenu(page, "Product", "Category (groups)", "Delete");
  await fieldMenu(page, "Sales", "Amount (bins)", "Delete");
  await expect(fieldRow(page, "Sales", "Amount (bins)")).toHaveCount(0);
});
