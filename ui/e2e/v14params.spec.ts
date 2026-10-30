// v1.4 parameters. What-if: New parameter… makes a table of values, a measure of the one
// picked and a slicer; picking a value in the slicer changes what the measure returns.
// Fields: a field parameter in a chart shows the measures picked in its slicer, or all
// of them. Calculation groups: a table of a group's items shows each applied to a
// measure, with the measure itself in the total row. The tables are deleted again at
// the end.
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
  await tabs.last().getByRole("tab").dblclick();
  await page.getByRole("textbox", { name: "Page name" }).fill(name);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("tab", { name })).toHaveAttribute("aria-selected", "true");
}

const fieldsPane = (page: Page) => page.locator(".fields-pane");
const visuals = (page: Page) => page.getByTestId("visual");

async function tick(page: Page, table: string, field: string) {
  const btn = fieldsPane(page).locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) });
  if ((await btn.locator("xpath=ancestor::li[1]").getAttribute("aria-expanded")) !== "true") await btn.click();
  const tableItem = fieldsPane(page).locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) }) });
  await tableItem.locator(".field-row", { has: page.locator(".field-name", { hasText: new RegExp(`^${field}$`) }) }).locator("input").click();
}

async function addVisual(page: Page, label: string, at: { x: number; y: number }): Promise<Locator> {
  const box = (await page.getByTestId("report-page").boundingBox())!;
  const scale = box.width / 1280;
  await page.mouse.click(box.x + at.x * scale, box.y + at.y * scale);
  const before = await visuals(page).count();
  await page.locator(".viz-gallery").getByRole("button", { name: label, exact: true }).click();
  await expect(visuals(page)).toHaveCount(before + 1);
  return visuals(page).nth(before);
}

type ChartOption = { series: { name?: string }[] };
const seriesNames = (visual: Locator) =>
  visual.locator(".cartesian").evaluate((el) => ((el as unknown as { chart?: { getOption(): ChartOption } }).chart?.getOption().series ?? []).map((x) => x.name ?? ""));

test.setTimeout(180_000);
test.use({ actionTimeout: 15_000 });

test("a what-if parameter: its slicer sets a measure's value", async ({ page }) => {
  await openSample(page);
  await newPage(page, "What if");

  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Sales$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New parameter…" }).click();
  const dialog = page.getByRole("dialog", { name: "New parameter" });
  await dialog.getByLabel("Name", { exact: true }).fill("Discount");
  await dialog.getByLabel("Maximum").fill("-5");
  await expect(dialog.getByRole("alert")).toContainText("maximum must be at least the minimum");
  await expect(dialog.getByRole("button", { name: "OK" })).toBeDisabled();
  await dialog.getByLabel("Maximum").fill("50");
  await dialog.getByLabel("Increment").fill("5");
  await dialog.getByLabel("Default").fill("10");
  await expect(dialog.getByRole("alert")).toHaveCount(0);
  await dialog.getByRole("button", { name: "OK" }).click();
  await expect(dialog).toBeHidden();

  // The slicer lists the values, and the measure is the default until one is picked.
  const slicer = visuals(page).first();
  await expect(slicer.getByRole("checkbox", { name: "25", exact: true })).toBeVisible({ timeout: 15_000 });
  await expect(slicer.getByRole("checkbox", { name: "50", exact: true })).toBeVisible();
  const card = await addVisual(page, "Card", { x: 900, y: 120 });
  await tick(page, "Discount", "Discount Value");
  await expect(card.locator(".card-value")).toHaveText("10");
  await slicer.locator(".slicer-item", { hasText: /^25$/ }).click();
  await expect(card.locator(".card-value")).toHaveText("25");
  if (shots) await page.screenshot({ path: `${shots}/v14-04-what-if.png` });

  // Leave the shared sample as other specs expect it.
  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Discount$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete table", exact: true }).click();
  await expect(fieldsPane(page).locator(".field-table-btn", { hasText: /^Discount$/ })).toHaveCount(0);
});

test("a field parameter: a chart shows the measures picked in its slicer", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Fields");

  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Sales$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New parameter…" }).click();
  const dialog = page.getByRole("dialog", { name: "New parameter" });
  await dialog.getByLabel("Kind").selectOption({ label: "Fields" });
  await dialog.getByLabel("Name", { exact: true }).fill("Metric");
  await expect(dialog.getByRole("button", { name: "OK" })).toBeDisabled();
  const offer = (name: string) => dialog.locator(".group-values li", { hasText: new RegExp(`^${name} Sales`) }).getByRole("checkbox");
  await offer("Units").check();
  await offer("Total Sales").check();
  await dialog.getByRole("button", { name: "OK" }).click();
  await expect(dialog).toBeHidden();

  // The slicer lists the labels in the order picked; the chart shows both measures.
  const slicer = visuals(page).first();
  await expect(slicer.locator(".slicer-item")).toHaveText(["Select all", "Units", "Total Sales"], { timeout: 15_000 });
  const chart = await addVisual(page, "Clustered column chart", { x: 800, y: 300 });
  await tick(page, "Product", "Category");
  await tick(page, "Metric", "Metric");
  await expect.poll(() => seriesNames(chart)).toEqual(["Units", "Total Sales"]);
  // Its well keeps the parameter.
  await expect(page.locator('.well[data-role="values"] .chip-name')).toHaveText(["Metric"]);

  // Picking one in the slicer leaves that one.
  await slicer.locator(".slicer-item", { hasText: /^Total Sales$/ }).click();
  await expect.poll(() => seriesNames(chart)).toEqual(["Total Sales"]);
  if (shots) await page.screenshot({ path: `${shots}/v14-05-field-parameter.png` });

  // Leave the shared sample as other specs expect it.
  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Metric$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete table", exact: true }).click();
  await expect(fieldsPane(page).locator(".field-table-btn", { hasText: /^Metric$/ })).toHaveCount(0);
});

test("a calculation group: each item applied to a measure", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Scenarios");

  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Sales$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New calculation group…" }).click();
  const dialog = page.getByRole("dialog", { name: "New calculation group" });
  await dialog.getByLabel("Name", { exact: true }).fill("Scenario");
  await dialog.getByLabel("Item name").fill("Base");
  await dialog.getByRole("button", { name: "Add item" }).click();
  await dialog.getByLabel("Item name").fill("Double");
  const editor = dialog.locator(".dax-editor");
  await expect(dialog.locator(".monaco-editor .view-lines")).toContainText("SELECTEDMEASURE");
  await editor.click();
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Delete");
  // SELECTEDMEASURE works here; a misspelt function is underlined.
  await page.keyboard.type("SELECTEDMEASURES ( ) * 2");
  await expect(dialog.locator(".squiggly-error")).toBeVisible({ timeout: 15_000 });
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Delete");
  await page.keyboard.type("SELECTEDMEASURE ( ) * 2");
  await expect(dialog.locator(".squiggly-error")).toHaveCount(0, { timeout: 15_000 });
  await dialog.getByRole("button", { name: "OK" }).click();
  await expect(dialog).toBeHidden();
  const group = fieldsPane(page).locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: /^Scenario$/ }) });
  await expect(group.locator(".table-icon-group")).toHaveCount(1);

  // A table of the items and Units: Double is twice Base, the total is Units itself.
  await addVisual(page, "Table", { x: 400, y: 300 });
  await tick(page, "Scenario", "Name");
  await tick(page, "Sales", "Units");
  const table = visuals(page).filter({ has: page.locator(".table-visual") });
  const rows = table.locator("tbody tr");
  await expect(rows).toHaveCount(2, { timeout: 15_000 });
  await expect(rows.locator("td:first-child")).toHaveText(["Base", "Double"]);
  const num = async (cell: Locator) => Number((await cell.textContent())!.replace(/[^\d.-]/g, ""));
  const base = await num(rows.nth(0).locator("td").nth(1));
  expect(base).toBeGreaterThan(0);
  expect(await num(rows.nth(1).locator("td").nth(1))).toBe(base * 2);
  expect(await num(table.locator("tfoot td").nth(1))).toBe(base);
  if (shots) await page.screenshot({ path: `${shots}/v14-06-calculation-group.png` });

  // Its items are edited in the same dialog.
  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Scenario$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Edit calculation group…" }).click();
  const edit = page.getByRole("dialog", { name: "Edit calculation group" });
  await edit.getByRole("button", { name: "Double" }).click();
  await edit.getByLabel("Item name").fill("Twice");
  await edit.getByRole("button", { name: "OK" }).click();
  await expect(edit).toBeHidden();
  await expect(rows.locator("td:first-child")).toHaveText(["Base", "Twice"]);

  // Leave the shared sample as other specs expect it.
  await fieldsPane(page).locator(".field-table-btn", { hasText: /^Scenario$/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete table", exact: true }).click();
  await expect(fieldsPane(page).locator(".field-table-btn", { hasText: /^Scenario$/ })).toHaveCount(0);
});
