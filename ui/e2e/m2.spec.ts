import { expect, type Locator, type Page, test } from "@playwright/test";
import { readFileSync, readdirSync } from "node:fs";

const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const shots = process.env.CHARTED_SHOTS;
const project = `${tmp}/docs/charted/Harbor & Pine`;

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  if (await explore.isVisible()) await explore.click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
}

const fieldsPane = (page: Page) => page.locator(".fields-pane");
const visuals = (page: Page) => page.getByTestId("visual");

async function expandTable(page: Page, table: string) {
  const btn = fieldsPane(page).locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) });
  if ((await btn.locator("xpath=ancestor::li[1]").getAttribute("aria-expanded")) !== "true") await btn.click();
}

/** Ticks a field in the Fields pane (adds it to the selected visual, or makes one). */
async function tick(page: Page, table: string, field: string) {
  await expandTable(page, table);
  const tableItem = fieldsPane(page).locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) }) });
  await tableItem.locator(".field-row", { has: page.locator(".field-name", { hasText: new RegExp(`^${field.replace(/[%]/g, "\\$&")}$`) }) }).locator("input").click();
}

/** Adds a visual of `label` type at a page position and returns it. */
async function addVisual(page: Page, label: string, at: { x: number; y: number }): Promise<Locator> {
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  const scale = pageBox.width / 1280;
  // Clear the selection and put the pointer where the visual should go.
  await page.mouse.click(pageBox.x + at.x * scale, pageBox.y + at.y * scale);
  const before = await visuals(page).count();
  await page.locator(".viz-gallery").getByRole("button", { name: label, exact: true }).click();
  await expect(visuals(page)).toHaveCount(before + 1);
  return visuals(page).nth(before);
}

test.setTimeout(120_000);

test("build the Overview page: card, column, line, table, a DIVIDE measure, a page filter, align, save and reopen", async ({ page }) => {
  await openSample(page);
  await expect(page.getByRole("tab", { name: "Page 1" })).toBeVisible();

  // Rename the page.
  await page.getByRole("tab", { name: "Page 1" }).dblclick();
  await page.getByRole("textbox", { name: "Page name" }).fill("Overview");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();

  // A new measure with DIVIDE, typed in the formula bar.
  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await page.keyboard.press("Control+m");
  const formula = page.getByRole("textbox", { name: "Measure formula" });
  await expect(formula).toBeVisible();
  await formula.fill("Margin % = DIVIDE ( [Total Sales] - [Total Cost, [Total Sales] )");
  await expect(page.locator(".formula-problem")).toBeVisible();
  await formula.fill("Margin % = DIVIDE ( [Total Sales] - [Total Cost], [Total Sales] )");
  await expect(page.locator(".formula-problem")).toBeHidden();
  await formula.press("Enter");
  await expect(fieldsPane(page).locator(".field-name", { hasText: /^Margin %$/ })).toBeVisible();
  await page.getByRole("group", { name: "Formula bar" }).getByRole("combobox", { name: "Format" }).selectOption({ label: "Percentage" });
  if (shots) await page.screenshot({ path: `${shots}/m2-measure.png` });
  await page.getByRole("button", { name: "Close formula bar" }).click();

  // Card: Total Sales.
  const card = await addVisual(page, "Card", { x: 140, y: 90 });
  await tick(page, "Sales", "Total Sales");
  await expect(card.locator(".card-value")).toHaveText(/^\$\d+(\.\d+)?M$/);

  // Card: Margin %.
  const margin = await addVisual(page, "Card", { x: 420, y: 90 });
  await tick(page, "Sales", "Margin %");
  await expect(margin.locator(".card-value")).toHaveText(/^\d+\.\d%$/);

  // Clustered column: Total Sales by Category.
  const column = await addVisual(page, "Clustered column chart", { x: 300, y: 380 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect(column.locator(".visual-title")).toHaveText("Total Sales by Category");
  await expect(column.locator("canvas").first()).toBeVisible();

  // Line: Total Sales by YearMonth.
  const line = await addVisual(page, "Line chart", { x: 900, y: 380 });
  await tick(page, "Date", "YearMonth");
  await tick(page, "Sales", "Total Sales");
  await expect(line.locator(".visual-title")).toHaveText("Total Sales by YearMonth");

  // Table: Region, Total Sales, Margin %.
  const table = await addVisual(page, "Table", { x: 960, y: 110 });
  await tick(page, "Store", "Region");
  await tick(page, "Sales", "Total Sales");
  await tick(page, "Sales", "Margin %");
  await expect(table.locator("thead th")).toHaveText(["Region", "Total Sales", "Margin %"]);
  await expect(table.locator("tfoot td").first()).toHaveText("Total");
  const totalBefore = await card.locator(".card-value").textContent();

  // Page filter: Year = 2025, by dragging Year into "Filters on this page".
  await page.locator(".canvas-scroller").click({ position: { x: 8, y: 8 } });
  await expandTable(page, "Date");
  const year = fieldsPane(page).locator(".field-row", { has: page.locator(".field-name", { hasText: /^Year$/ }) });
  await year.dragTo(page.getByRole("group", { name: "Filters on this page" }).locator(".filter-drop"));
  const yearCard = page.getByRole("group", { name: "Filters on this page" }).locator(".filter-card", { hasText: "Year" });
  await yearCard.locator(".filter-card-toggle").click();
  await yearCard.getByRole("checkbox", { name: "2025" }).click();
  await expect(yearCard.locator(".filter-card-summary")).toHaveText("is 2025");
  await expect(card.locator(".card-value")).not.toHaveText(totalBefore!);
  if (shots) await page.screenshot({ path: `${shots}/m2-filter.png` });

  // Align the two cards to the top, then check their positions.
  await card.click({ position: { x: 20, y: 20 } });
  await margin.click({ position: { x: 20, y: 20 }, modifiers: ["Shift"] });
  await expect(page.getByTestId("status-page")).toContainText("2 selected");
  await page.getByRole("button", { name: "Align top" }).click();
  const tops = await Promise.all([card, margin].map(async (v) => (await v.boundingBox())!.y));
  expect(Math.abs(tops[0]! - tops[1]!)).toBeLessThan(1);

  // Move a visual by dragging it, and undo the move.
  await page.keyboard.press("Escape");
  const box = (await column.boundingBox())!;
  await column.click({ position: { x: 30, y: 60 } });
  await page.mouse.move(box.x + 30, box.y + 60);
  await page.mouse.down();
  await page.mouse.move(box.x + 80, box.y + 90, { steps: 8 });
  await page.mouse.up();
  await expect.poll(async () => (await column.boundingBox())!.x).toBeGreaterThan(box.x + 20);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => Math.round((await column.boundingBox())!.x)).toBe(Math.round(box.x));
  if (shots) await page.screenshot({ path: `${shots}/m2-overview.png` });

  // Save, close and reopen: the report is identical.
  await page.keyboard.press("Control+s");
  await expect(page.getByText("All changes saved")).toBeVisible();
  const pagesDir = `${project}/report/pages`;
  const snapshot = () => readdirSync(pagesDir, { recursive: true }).sort().map(String);
  const files = snapshot();
  expect(files.filter((f) => f.endsWith(".json"))).toHaveLength(1 + 5);
  const yaml = readFileSync(`${project}/model.ossie.yaml`, "utf8");
  expect(yaml).toContain("name: Margin %");
  expect(yaml).toContain("DIVIDE ( [Total Sales] - [Total Cost], [Total Sales] )");
  expect(yaml).toContain("OSSIE_SQL_2026");

  const titles = async () => (await page.locator(".visual-title").allTextContents()).sort();
  const before = { titles: await titles(), card: await card.locator(".card-value").textContent() };
  await page.keyboard.press("Control+w");
  await expect(page.getByRole("button", { name: "Explore the sample" })).toBeVisible();
  await page.getByRole("button", { name: /Harbor & Pine/ }).first().click();
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible({ timeout: 30_000 });
  await expect(visuals(page)).toHaveCount(5);
  await expect.poll(titles).toEqual(before.titles);
  await expect(visuals(page).first().locator(".card-value")).toHaveText(before.card!);
  expect(snapshot()).toEqual(files);
});

