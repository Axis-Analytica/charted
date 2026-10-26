import { expect, type Page, test } from "@playwright/test";

const shots = process.env.CHARTED_SHOTS;

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

const steps = (page: Page) => page.locator(".qe-step .qe-step-name");
const header = (page: Page, name: string) => page.locator(".qe-preview th .qe-name", { hasText: new RegExp(`^${name}$`) });

/** Pastes text into a field, as a paste from a spreadsheet does. */
async function paste(page: Page, selector: ReturnType<Page["locator"]>, text: string) {
  await selector.evaluate((el, t) => {
    const data = new DataTransfer();
    data.setData("text/plain", t);
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, text);
}

/** Ticks a table's field in the Fields pane (expanding the table first). */
async function tick(page: Page, table: string, field: string) {
  const fields = page.locator(".fields-pane");
  const item = fields.locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) }) });
  if ((await item.getAttribute("aria-expanded")) !== "true") await item.locator(".field-table-btn").first().click();
  await item.locator(".field-row", { has: page.locator(".field-name", { hasText: new RegExp(`^${field}$`) }) }).locator("input").click();
}

test.setTimeout(120_000);
test.use({ actionTimeout: 15_000 });

test("reference a query, transform it and apply", async ({ page }) => {
  await openSample(page);
  await page.keyboard.press("Control+4");
  await expect(page.locator(".qe-query").first()).toBeVisible();

  // Reference Store: a new query starting from Store's result.
  await page.getByRole("button", { name: "Options for Store", exact: true }).click();
  await page.getByRole("menuitem", { name: "Reference" }).click();
  await expect(page.locator(".qe-query.is-on")).toContainText("Store (2)");
  await expect(steps(page)).toHaveText(["Source"]);
  await expect(page.locator(".qe-step .qe-step-link")).toHaveText("Store");
  await expect(header(page, "Region")).toBeVisible();
  await page.locator(".qe-code-tabs").getByRole("tab", { name: "M" }).click();
  await expect(page.locator(".qe-code-text")).toContainText("Source = Store");

  // Keep the regions, once each, and name the query.
  await page.getByRole("button", { name: "Column options for Region" }).click();
  await page.getByRole("menuitem", { name: "Remove other columns" }).click();
  await page.getByRole("button", { name: "Column options for Region" }).click();
  await page.getByRole("menuitem", { name: "Remove duplicates" }).click();
  await expect(steps(page)).toHaveText(["Source", "Removed Other Columns", "Removed Duplicates"]);
  const regions = await page.locator(".qe-preview tbody tr").count();
  expect(regions).toBeGreaterThan(1);
  const name = page.getByRole("textbox", { name: "Query name" });
  await name.fill("Regions");
  await name.press("Enter");
  await expect(page.locator(".qe-query.is-on")).toContainText("Regions");
  // Store can't be deleted while Regions starts from it.
  await page.getByRole("button", { name: "Options for Store", exact: true }).click();
  await expect(page.getByRole("menuitem", { name: "Delete" })).toBeDisabled();
  await page.keyboard.press("Escape");
  await shot(page, "v11-01-reference");

  // The source links to Store.
  await page.locator(".qe-step .qe-step-link").click();
  await expect(page.locator(".qe-query.is-on")).toContainText(/^Store$/);

  await page.getByRole("button", { name: "Close & Apply" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".fields-pane .field-table-btn", { hasText: /^Regions$/ })).toBeVisible();

  // A change to Store reaches the query referencing it.
  await page.keyboard.press("Control+4");
  await page.locator(".qe-query", { hasText: /^Store$/ }).click();
  await page.getByRole("button", { name: "Column options for Region" }).click();
  await page.getByRole("menuitem", { name: "Replace values…" }).click();
  const d = page.getByRole("dialog");
  await d.getByRole("textbox", { name: "Value to find" }).fill("North America");
  await d.getByRole("textbox", { name: "Replace with" }).fill("Americas");
  await d.getByRole("button", { name: "OK" }).click();
  await page.locator(".qe-query", { hasText: /^Regions$/ }).click();
  await expect(page.locator(".qe-preview tbody")).toContainText("Americas");
  await page.getByRole("button", { name: "Close & Apply" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });

  // The loaded table has it: a slicer on Regions[Region] lists Americas.
  await page.getByRole("button", { name: "New page" }).click();
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  await page.mouse.click(pageBox.x + 100, pageBox.y + 100);
  await page.locator(".viz-gallery").getByRole("button", { name: "Slicer", exact: true }).click();
  await tick(page, "Regions", "Region");
  const slicer = page.getByTestId("visual").filter({ has: page.locator(".slicer") });
  await expect(slicer.locator(".slicer-item", { hasText: "Americas" })).toBeVisible();
  await expect(slicer.locator(".slicer-item", { hasText: "North America" })).toHaveCount(0);
});

test("enter data, apply and use the table in a visual", async ({ page }) => {
  await openSample(page);
  await page.keyboard.press("Control+4");
  await page.locator(".qe-ribbon").getByRole("button", { name: "New source" }).click();
  await page.getByRole("menuitem", { name: "Enter data…" }).click();
  const d = page.getByRole("dialog", { name: "Enter data" });
  await d.getByRole("textbox", { name: "Table name" }).fill("Region goals");

  // Paste a block with its header row into the first column's name.
  await paste(page, d.getByRole("textbox", { name: "Name of column 1" }), "Region\tTarget\r\nEurope\t100\r\nAsia Pacific\t250\r\n");
  await expect(d.getByRole("textbox", { name: "Name of column 2" })).toHaveValue("Target");
  await expect(d.getByRole("textbox", { name: "Row 2, Target" })).toHaveValue("250");
  await d.getByRole("combobox", { name: "Type of Target" }).selectOption("Int64");
  // One more row, typed in.
  await d.getByRole("button", { name: "Add row" }).click();
  await d.getByRole("textbox", { name: "Row 3, Region" }).fill("North America");
  await d.getByRole("textbox", { name: "Row 3, Target" }).fill("400");
  // A column added and removed again.
  await d.getByRole("button", { name: "Add column" }).click();
  await expect(d.getByRole("textbox", { name: "Name of column 3" })).toHaveValue("Column3");
  await d.getByRole("button", { name: "Remove column Column3" }).click();
  await expect(d.getByRole("textbox", { name: "Name of column 3" })).toHaveCount(0);
  await shot(page, "v11-02-enter-data");
  await d.getByRole("button", { name: "OK" }).click();
  await expect(d).toBeHidden();

  await expect(page.locator(".qe-query.is-on")).toContainText("Region goals");
  await expect(page.locator(".qe-status")).toContainText("2 columns, 3 rows");
  await expect(page.locator(".qe-preview tbody")).toContainText("Asia Pacific");
  await page.locator(".qe-code-tabs").getByRole("tab", { name: "M" }).click();
  await expect(page.locator(".qe-code-text")).toContainText("Binary.Decompress(Binary.FromText(");

  // Editing the source opens the table again.
  await page.getByRole("button", { name: "Change the source" }).click();
  await expect(page.getByRole("dialog", { name: "Enter data" }).getByRole("textbox", { name: "Row 1, Region" })).toHaveValue("Europe");
  await page.getByRole("dialog", { name: "Enter data" }).getByRole("button", { name: "Cancel" }).click();

  await page.getByRole("button", { name: "Close & Apply" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });

  // A table visual of Region and Target, with its total.
  await page.getByRole("button", { name: "New page" }).click();
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  await page.mouse.click(pageBox.x + 100, pageBox.y + 100);
  await page.locator(".viz-gallery").getByRole("button", { name: "Table", exact: true }).click();
  await tick(page, "Region goals", "Region");
  await tick(page, "Region goals", "Target");
  const visual = page.getByTestId("visual").filter({ has: page.locator(".table-visual") });
  await expect(visual).toContainText("Asia Pacific");
  await expect(visual).toContainText("750");
  await shot(page, "v11-03-visual");
});
