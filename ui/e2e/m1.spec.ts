import { expect, type Page, test } from "@playwright/test";

const shots = process.env.CHARTED_SHOTS;

test.use({ permissions: ["clipboard-read", "clipboard-write"] });

/** A table entry in the sidebar, by exact name. */
const tableItem = (page: Page, name: string) => page.getByRole("tree", { name: "Tables" }).locator(".tree-label").getByText(name, { exact: true });

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  if (await explore.isVisible()) await explore.click();
  await expect(tableItem(page, "Sales")).toBeVisible({ timeout: 30_000 });
}

/** Selects a column through the Fields pane (expanding its table first). */
async function selectColumn(page: Page, table: string, column: string) {
  const item = page.getByRole("tree", { name: "Tables" }).getByRole("treeitem").filter({ has: page.locator(".tree-label").getByText(table, { exact: true }) }).first();
  if ((await item.getAttribute("aria-expanded")) !== "true") await item.locator(".tree-chevron").click();
  await item.getByRole("group").locator(".tree-leaf").filter({ has: page.locator(".tree-label").getByText(column, { exact: true }) }).click();
  await expect(page.getByRole("complementary", { name: "Column properties" }).locator(".props-title")).toHaveText(column);
}

const clipboard = (page: Page) => page.evaluate(() => navigator.clipboard.readText());

test("data view: filter, multi-sort, copy, statistics, format with undo", async ({ page }) => {
  await openSample(page);
  await expect(page.getByRole("heading", { name: "Sales" })).toBeVisible();
  await expect(page.locator(".preview-sub")).toContainText("245,387 rows");
  if (shots) await page.screenshot({ path: `${shots}/m1-data-view.png` });

  // Filter Channel to Online through the value checklist.
  await selectColumn(page, "Sales", "Channel");
  await expect(page.getByTestId("status-distinct")).toContainText(/Channel: \d+ distinct values/);
  await page.keyboard.press("Alt+ArrowDown");
  const menu = page.getByRole("dialog", { name: "Sort and filter Channel" });
  await expect(menu).toBeVisible();
  await menu.getByRole("option", { name: /\(Select all\)/ }).click();
  await menu.getByRole("option", { name: /^Online/ }).click();
  if (shots) await page.screenshot({ path: `${shots}/m1-filter-menu.png` });
  await menu.getByRole("button", { name: "Apply" }).click();
  await expect(menu).toBeHidden();
  await expect(page.locator(".chip.is-filter")).toContainText("Channel: Online");
  await expect(page.locator(".preview-sub")).toContainText(/[\d,]+ of 245,387 rows/);
  await expect(page.getByTestId("status-rows")).toContainText(/of 245,387 rows/);

  // Copy the filtered column: every row is Online.
  await page.keyboard.press("Control+c");
  await expect(page.getByRole("status").filter({ hasText: /Copied [\d,]+ rows/ })).toBeVisible();
  const lines = (await clipboard(page)).split("\n");
  expect(lines[0]).toBe("Channel");
  expect(lines.length).toBeGreaterThan(1000);
  expect(new Set(lines.slice(1))).toEqual(new Set(["Online"]));

  // Sort by Quantity descending, then UnitPrice ascending (Shift adds to the sort).
  await selectColumn(page, "Sales", "Quantity");
  await page.keyboard.press("Alt+ArrowDown");
  await page.getByRole("dialog", { name: "Sort and filter Quantity" }).getByRole("menuitem", { name: "Sort descending" }).click();
  await selectColumn(page, "Sales", "UnitPrice");
  await page.keyboard.press("Alt+ArrowDown");
  await page.getByRole("dialog", { name: "Sort and filter UnitPrice" }).getByRole("menuitem", { name: "Sort ascending" }).click({ modifiers: ["Shift"] });
  const chips = page.locator(".chips .chip:not(.is-filter)");
  await expect(chips).toHaveCount(2);
  await expect(chips.nth(0)).toContainText("Quantity ↓");
  await expect(chips.nth(1)).toContainText("UnitPrice ↑");
  await selectColumn(page, "Sales", "Quantity");
  await page.keyboard.press("Control+c");
  await expect.poll(async () => (await clipboard(page)).split("\n")[0]).toBe("Quantity");
  const qty = (await clipboard(page)).split("\n").slice(1).map(Number);
  expect(qty.every((q, i) => i === 0 || qty[i - 1]! >= q)).toBe(true);
  if (shots) await page.screenshot({ path: `${shots}/m1-multisort.png` });

  // Statistics drawer for UnitPrice.
  await selectColumn(page, "Sales", "UnitPrice");
  await page.keyboard.press("Control+i");
  const stats = page.getByRole("region", { name: "Column statistics" });
  await expect(stats.locator(".stats-title")).toHaveText("UnitPrice");
  for (const label of ["Values", "Distinct", "Mean", "Median", "Std. deviation"]) await expect(stats.getByText(label, { exact: true })).toBeVisible();
  await expect(stats.getByRole("figure", { name: "Distribution" })).toBeVisible();
  await expect(stats.locator(".hist-bar")).toHaveCount(20);
  if (shots) await page.screenshot({ path: `${shots}/m1-stats.png` });

  // Change the format, see the preview, undo.
  const props = page.getByRole("complementary", { name: "Column properties" });
  const formatInput = props.getByRole("textbox", { name: "Format string" });
  const original = await formatInput.inputValue();
  await formatInput.fill("#,0.0");
  await expect(props.locator(".props-preview")).toContainText("1,234.6");
  await formatInput.press("Enter");
  await expect(page.getByText("Unsaved changes").first()).toBeVisible();
  await expect(props.getByRole("textbox", { name: "Format string" })).toHaveValue("#,0.0");
  if (shots) await page.screenshot({ path: `${shots}/m1-format.png` });
  await page.locator(".tree").focus();
  await page.keyboard.press("Control+z");
  await expect(props.getByRole("textbox", { name: "Format string" })).toHaveValue(original);

  // Clearing filters restores the full row count.
  await page.getByRole("button", { name: "Clear filters" }).first().click();
  await expect(page.locator(".preview-sub")).toContainText("245,387 rows");
});

test("find, type checks and the frame readout", async ({ page }) => {
  await openSample(page);
  await tableItem(page, "Store").click();
  await expect(page.getByRole("heading", { name: "Store" })).toBeVisible();

  await page.keyboard.press("Control+f");
  const find = page.getByRole("searchbox", { name: "Find in table" }).or(page.getByRole("textbox", { name: "Find in table" }));
  await find.fill("north");
  await find.press("Enter");
  await expect(page.locator(".findbar-status")).toContainText(/\d+ matching rows?/);
  if (shots) await page.screenshot({ path: `${shots}/m1-find.png` });
  await find.press("Escape");
  await expect(page.locator(".findbar")).toBeHidden();

  // Text that isn't a number can't become a whole number.
  await selectColumn(page, "Store", "City");
  await page.getByRole("complementary", { name: "Column properties" }).getByRole("combobox", { name: "Data type" }).selectOption("Int64");
  await expect(page.getByRole("alert")).toContainText("don't convert");

  await page.keyboard.press("Control+Alt+p");
  await expect(page.getByTestId("frame-hud")).toContainText("fps");
});
