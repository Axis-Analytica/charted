import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { expect, type Locator, type Page, test } from "@playwright/test";

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

test.setTimeout(240_000);
test.use({ actionTimeout: 15_000 });

test("search visuals and slicers, and the Fields pane's menu: quick measures, rename, hide, delete", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Search");

  // Visuals: type to find one, Enter adds it.
  await page.getByTestId("report-page").click({ position: { x: 600, y: 400 } });
  const search = page.getByRole("textbox", { name: "Search visuals" });
  await search.fill("doughnut");
  await expect(page.locator(".viz-gallery .viz-type")).toHaveCount(1);
  await expect(page.locator(".viz-gallery").getByRole("button", { name: "Donut chart" })).toBeVisible();
  await search.press("Enter");
  await expect(visuals(page)).toHaveCount(1);
  await expect(search).toHaveValue("");
  await search.fill("zzz");
  await expect(page.locator(".viz-none")).toContainText("No visual matches");
  await search.press("Escape");
  await expect(page.locator(".viz-gallery .viz-type").first()).toBeVisible();

  // A slicer of 20,000 customers lists 5,000; its search finds any of them.
  const slicer = await addVisual(page, "Slicer", { x: 200, y: 200 });
  await tick(page, "Customer", "Customer");
  const box = slicer.getByRole("textbox", { name: /^Search/ });
  await expect(box).toBeVisible();
  await box.fill("son");
  const items = slicer.locator(".slicer-item[role=option] span");
  await expect(items.first()).toBeVisible();
  await expect.poll(async () => (await items.allTextContents()).every((x) => x.toLowerCase().includes("son"))).toBe(true);
  const found = await items.count();
  await slicer.getByText("Select all results").click();
  await expect(slicer.locator(".slicer-item.is-on")).toHaveCount(found);
  await expect(slicer.getByRole("button", { name: "Clear selections" })).toBeVisible();
  await shot(page, "v1-01-slicer-search");
  await box.fill("");
  await expect(slicer.getByText("Select all", { exact: true })).toBeVisible();

  // Right-click a table: a quick measure.
  await expandTable(page, "Sales");
  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Sales$/ }).click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Sales" });
  await expect(menu.getByRole("menuitem", { name: "New measure" })).toBeVisible();
  await shot(page, "v1-02-fields-menu");
  await menu.getByRole("menuitem", { name: "New quick measure…" }).click();
  const dialog = page.getByRole("dialog", { name: "Quick measure" });
  await dialog.getByRole("combobox", { name: "Calculation" }).selectOption({ label: "Year-over-year change %" });
  await dialog.getByRole("combobox", { name: "Value" }).selectOption({ label: "Total Sales" });
  await expect(dialog.locator(".quick-dax")).toContainText("SAMEPERIODLASTYEAR");
  await shot(page, "v1-03-quick-measure");
  await dialog.getByRole("button", { name: "Add" }).click();
  await expect(dialog).toBeHidden();
  await expect(fieldRow(page, "Sales", "Total Sales YoY %")).toBeVisible();
  await expect(page.locator(".formula-bar")).toBeVisible();

  // Rename, hide and unhide, delete and undo, from the field's menu.
  await fieldRow(page, "Sales", "Total Sales YoY %").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Rename" }).click();
  await page.getByRole("textbox", { name: "New name" }).fill("Sales growth");
  await page.keyboard.press("Enter");
  await expect(fieldRow(page, "Sales", "Sales growth")).toBeVisible();
  await fieldRow(page, "Sales", "Sales growth").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Hide" }).click();
  await expect(fieldRow(page, "Sales", "Sales growth")).toHaveCount(0);
  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Sales$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "View hidden fields" }).click();
  await expect(fieldRow(page, "Sales", "Sales growth")).toHaveClass(/is-hidden/);
  await fieldRow(page, "Sales", "Sales growth").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Unhide" }).click();
  await expect(fieldRow(page, "Sales", "Sales growth")).not.toHaveClass(/is-hidden/);
  // The keyboard opens the menu too.
  await fieldRow(page, "Sales", "Sales growth").focus();
  await page.keyboard.press("Shift+F10");
  await expect(page.getByRole("menu", { name: "Sales growth" })).toBeVisible();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await expect(fieldRow(page, "Sales", "Sales growth")).toHaveCount(0);
  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await page.keyboard.press("Control+z");
  await expect(fieldRow(page, "Sales", "Sales growth")).toBeVisible();

  // New column goes to the Data view with the formula open.
  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Product$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New column" }).click();
  await expect(page.getByRole("group", { name: "Column formula bar" })).toBeVisible();
});

const pick = (page: Page, path: string) => page.evaluate((p) => ((window as unknown as { __chartedPick: () => string }).__chartedPick = () => p), path);

test("a Western European, semicolon-separated CSV reads right, and its settings can be changed", async ({ page }) => {
  // Written by an older Windows program: Latin-1 text, semicolons, decimal commas.
  const file = `${tmpdir()}/charted-e2e-prices.csv`;
  writeFileSync(file, Buffer.from("Produit;Prix\nCafé crème;2,50\nThé glacé;1.234,75\n", "latin1"));
  await openSample(page);
  await page.keyboard.press("Control+4");
  await expect(page.locator(".qe")).toBeVisible();
  await pick(page, file);
  await page.locator(".qe-ribbon").getByRole("button", { name: "New source" }).click();
  await page.getByRole("menuitem", { name: "Text/CSV" }).click();
  const body = page.locator(".qe-preview tbody");
  await expect(body.locator("tr").first()).toContainText("Café crème");
  await expect(body.locator("tr").nth(1).locator("td.is-num").last()).toHaveText(/1,234\.75|1234\.75/);
  await shot(page, "v1-04-latin1-csv");
  // The source's settings: a decimal point instead leaves the prices as text.
  await page.locator(".qe-steps li").first().getByRole("button", { name: "Change the source" }).click();
  const dialog = page.getByRole("dialog", { name: "Text/CSV settings" });
  await expect(dialog.getByRole("combobox", { name: "File encoding" })).toHaveValue("latin-1");
  await expect(dialog.getByRole("combobox", { name: "Decimal separator" })).toHaveValue(",");
  await dialog.getByRole("combobox", { name: "Decimal separator" }).selectOption({ label: "Point (1,234.5)" });
  await dialog.getByRole("button", { name: "OK" }).click();
  await expect(dialog).toBeHidden();
  await page.locator(".qe-steps li").first().getByRole("button", { name: "Change the source" }).click();
  await expect(dialog.getByRole("combobox", { name: "Decimal separator" })).toHaveValue(".");
  await dialog.getByRole("button", { name: "Cancel" }).click();
});
