import { expect, type Locator, type Page, test } from "@playwright/test";
import { readFileSync, writeFileSync } from "node:fs";

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

async function shot(page: Page, name: string) {
  if (shots) await page.screenshot({ path: `${shots}/${name}.png` });
}

const pick = (page: Page, path: string) => page.evaluate((p) => ((window as unknown as { __chartedPick: () => string }).__chartedPick = () => p), path);

/** Opens a toolbar group and chooses a transform. */
async function transform(page: Page, group: string, item: string) {
  await page.locator(".qe-ribbon").getByRole("button", { name: group, exact: true }).click();
  await page.getByRole("menuitem", { name: item }).click();
}

/** A column's header menu entry. */
async function columnAction(page: Page, column: string, item: string) {
  await page.getByRole("button", { name: `Column options for ${column}` }).click();
  await page.getByRole("menuitem", { name: item }).click();
}

const dialog = (page: Page) => page.getByRole("dialog");
const steps = (page: Page) => page.locator(".qe-step .qe-step-name");
const header = (page: Page, name: string): Locator => page.locator(".qe-preview th .qe-name", { hasText: new RegExp(`^${name}$`) });

async function ok(page: Page) {
  await dialog(page).getByRole("button", { name: "OK" }).click();
  await expect(dialog(page)).toBeHidden();
}

test.setTimeout(180_000);
test.use({ actionTimeout: 15_000 });

// A render loop (React's "Maximum update depth exceeded") fails the test.
let loops = 0;
test.beforeEach(({ page }) => {
  loops = 0;
  page.on("console", (m) => {
    if (m.type() === "error" && /Maximum update depth/.test(m.text())) loops += 1;
  });
});
test.afterEach(() => expect(loops, "React render loops").toBe(0));

test("clean stores_raw.csv, split and add a conditional column, merge into Store, reorder and undo steps, apply", async ({ page }) => {
  await openSample(page);

  // A slicer on Store[Region], to see the applied change reach the report.
  await page.getByRole("button", { name: "New page" }).click();
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  await page.mouse.click(pageBox.x + 100, pageBox.y + 100);
  await page.locator(".viz-gallery").getByRole("button", { name: "Slicer", exact: true }).click();
  const fields = page.locator(".fields-pane");
  const storeBtn = fields.locator(".field-table-btn", { hasText: /^Store$/ });
  if ((await storeBtn.locator("xpath=ancestor::li[1]").getAttribute("aria-expanded")) !== "true") await storeBtn.click();
  await fields.locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: /^Store$/ }) }).locator(".field-row", { has: page.locator(".field-name", { hasText: /^Region$/ }) }).locator("input").click();
  const slicer = page.getByTestId("visual").filter({ has: page.locator(".slicer") });
  await expect(slicer.locator(".slicer-item", { hasText: "North America" })).toBeVisible();

  // Transform data.
  await page.keyboard.press("Control+4");
  await expect(page.locator(".qe-query")).toHaveCount(6);
  await shot(page, "m5-01-editor");

  // Get the messy store list: charted promotes the first row and detects types, which
  // is wrong for this file (it has title rows), so those steps go.
  await pick(page, `${project}/data/stores_raw.csv`);
  await page.locator(".qe-ribbon").getByRole("button", { name: "New source" }).click();
  await page.getByRole("menuitem", { name: "Text/CSV" }).click();
  await expect(page.locator(".qe-query.is-on")).toContainText("stores_raw");
  await expect(steps(page)).toHaveText(["Source", "Promoted Headers"]);
  await header(page, "Harbor & Pine property register").waitFor();
  await shot(page, "m5-02-messy");
  await page.getByRole("button", { name: "Delete Promoted Headers" }).click();
  await expect(steps(page)).toHaveText(["Source"]);

  await transform(page, "Reduce rows", "Remove top rows…");
  await dialog(page).getByRole("spinbutton", { name: "Number of rows" }).fill("2");
  await ok(page);
  await transform(page, "Transform", "Use first row as headers");
  await expect(header(page, "store name")).toBeVisible();
  await transform(page, "Reduce rows", "Remove blank rows");
  await transform(page, "Reduce rows", "Remove duplicates…");
  await ok(page);
  await expect(page.locator(".qe-status")).toContainText("6 columns, 40 rows");

  await columnAction(page, "store name", "Format text…");
  await dialog(page).getByRole("group", { name: "Columns" }).getByLabel("format").check();
  await dialog(page).getByRole("combobox", { name: "Format" }).selectOption("trim");
  await ok(page);
  await columnAction(page, "store name", "Format text…");
  await dialog(page).getByRole("combobox", { name: "Format" }).selectOption("capitalize");
  await ok(page);
  await expect(page.locator(".qe-preview tbody tr").nth(1).locator("td").nth(2)).toHaveText("Auckland Pine Ridge");

  // Split "City, Country" and type the square feet (thousands separators and all).
  await columnAction(page, "location", "Split column by delimiter…");
  await dialog(page).getByRole("textbox", { name: "Delimiter" }).fill(", ");
  await dialog(page).getByRole("combobox", { name: "Split at" }).selectOption("left");
  await ok(page);
  await expect(header(page, "location.1")).toBeVisible();
  await page.getByRole("button", { name: "Change the type of square feet" }).click();
  await page.getByRole("menuitem", { name: "Whole number" }).click();
  await expect(page.locator(".qe-preview tbody tr").first().locator("td.is-num").last()).toHaveText("21000");

  // A conditional column from the size.
  await transform(page, "Add column", "Conditional column…");
  const d = dialog(page);
  await d.getByRole("textbox", { name: "New column name" }).fill("size band");
  await d.getByRole("combobox", { name: "Column" }).selectOption("square feet");
  await d.getByRole("combobox", { name: "Condition" }).selectOption("greaterOrEqual");
  await d.getByRole("textbox", { name: "Value" }).fill("24000");
  await d.getByRole("textbox", { name: "Output" }).fill("Large");
  await d.getByRole("textbox", { name: "Else" }).fill("Standard");
  await ok(page);
  await expect(header(page, "size band")).toBeVisible();
  await expect(page.locator(".qe-code-text")).toContainText("CASE WHEN");
  await page.locator(".qe-code-tabs").getByRole("tab", { name: "M" }).click();
  await expect(page.locator(".qe-code-text")).toContainText("Table.AddColumn(");
  await shot(page, "m5-03-cleaned");

  // Undo and redo the last step; select an earlier step; drag a step up.
  await page.keyboard.press("Control+z");
  await expect(header(page, "size band")).toHaveCount(0);
  await page.keyboard.press("Control+Shift+z");
  await expect(header(page, "size band")).toBeVisible();
  await steps(page).filter({ hasText: "Removed Duplicates" }).click();
  await expect(header(page, "location.1")).toHaveCount(0);
  await steps(page).filter({ hasText: "Added Conditional Column" }).click();
  const names = await steps(page).allTextContents();
  // Drag a step up, then move it back down with the keyboard.
  await page.locator(".qe-step", { hasText: "Removed Duplicates" }).dragTo(page.locator(".qe-step", { hasText: "Removed Blank Rows" }));
  await expect.poll(async () => (await steps(page).allTextContents()).indexOf("Removed Duplicates")).toBeLessThan(names.indexOf("Removed Duplicates"));
  await page.locator(".qe-step", { hasText: "Removed Duplicates" }).focus();
  await page.keyboard.press("Alt+ArrowDown");
  await expect.poll(async () => (await steps(page).allTextContents()).indexOf("Removed Duplicates")).toBe(names.indexOf("Removed Duplicates"));
  await expect(page.locator(".qe-status")).toContainText("40 rows");

  // It only feeds Store: no table of its own.
  await page.getByRole("checkbox", { name: "Load to report" }).uncheck();

  // Store: merge in the manager and size band, and rename a region.
  await page.locator(".qe-query", { hasText: /^Store$/ }).click();
  await transform(page, "Combine", "Merge queries…");
  await d.getByRole("group", { name: "Matching columns in this query" }).getByLabel("StoreCode").check();
  await d.getByRole("combobox", { name: "Merge with" }).selectOption({ label: "stores_raw" });
  await d.getByRole("group", { name: "Matching columns in that query" }).getByLabel("store code").check();
  await d.getByRole("group", { name: "Columns to add" }).getByLabel("manager").check();
  await d.getByRole("group", { name: "Columns to add" }).getByLabel("size band").check();
  await ok(page);
  await expect(header(page, "manager")).toBeVisible();
  await columnAction(page, "Region", "Replace values…");
  await d.getByRole("textbox", { name: "Value to find" }).fill("North America");
  await d.getByRole("textbox", { name: "Replace with" }).fill("Americas");
  await ok(page);
  await expect(page.locator(".qe-preview tbody")).toContainText("Americas");
  await shot(page, "m5-04-merged");

  // Close & Apply: the report's slicer shows the renamed region.
  await page.getByRole("button", { name: "Close & Apply" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
  await expect(slicer.locator(".slicer-item", { hasText: "Americas" })).toBeVisible();
  await expect(slicer.locator(".slicer-item", { hasText: "North America" })).toHaveCount(0);
  if ((await storeBtn.locator("xpath=ancestor::li[1]").getAttribute("aria-expanded")) !== "true") await storeBtn.click();
  await expect(fields.locator(".field-name", { hasText: /^manager$/ })).toBeVisible();
  await shot(page, "m5-05-applied");
});

test("Get Data from Excel, see a source change and refresh", async ({ page }) => {
  await openSample(page);
  await page.keyboard.press("Control+4");
  await pick(page, `${project}/data/targets.xlsx`);
  await page.locator(".qe-ribbon").getByRole("button", { name: "New source" }).click();
  await page.getByRole("menuitem", { name: "Excel workbook" }).click();
  const nav = page.getByRole("dialog", { name: "Navigator" });
  await expect(nav.getByText("Targets 2025")).toBeVisible();
  await nav.getByText("Targets 2025").hover();
  await expect(nav.locator(".qe-preview tbody tr").first()).toContainText("2025-01-01");
  await shot(page, "m5-06-navigator");
  await nav.getByRole("checkbox").nth(1).check();
  await nav.getByRole("button", { name: "Load" }).click();
  await expect(page.locator(".qe-query.is-on")).toContainText("Targets 2025");
  await page.getByRole("button", { name: "Close & Apply" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".fields-pane .field-table-btn", { hasText: /^Targets 2025$/ })).toBeVisible();

  // Change a source file: charted notices and offers to refresh.
  const raw = `${project}/data/stores_raw.csv`;
  writeFileSync(raw, readFileSync(raw, "utf8").replace("Ben Okafor", "Bea Okafor"));
  const banner = page.locator(".banner", { hasText: "Source data changed" });
  await expect(banner).toBeVisible({ timeout: 15_000 });
  await shot(page, "m5-07-source-changed");
  await banner.getByRole("button", { name: "Refresh" }).click();
  await expect(banner).toBeHidden({ timeout: 30_000 });
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.locator(".dirty-dot")).toHaveCount(0);
});
