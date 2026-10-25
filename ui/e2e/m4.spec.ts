import { expect, type Locator, type Page, test } from "@playwright/test";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";

const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const shots = process.env.CHARTED_SHOTS;
const themeDir = `${tmp}/home/.local/state/omarchy/current/theme`;

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  if (await explore.isVisible()) await explore.click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
}

/** A fresh page to work on (earlier specs leave visuals on Page 1). */
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

async function save(page: Page) {
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.locator(".dirty-dot")).toHaveCount(0);
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

async function tick(page: Page, table: string, field: string) {
  await expandTable(page, table);
  const tableItem = fieldsPane(page).locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) }) });
  await tableItem.locator(".field-row", { has: page.locator(".field-name", { hasText: new RegExp(`^${field}$`) }) }).locator("input").click();
}

/** Adds a visual of `label` type, sized by the gallery default, and moves it to `at`. */
async function addVisual(page: Page, label: string, at: { x: number; y: number }): Promise<Locator> {
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  const scale = pageBox.width / 1280;
  await page.mouse.click(pageBox.x + at.x * scale, pageBox.y + at.y * scale);
  const before = await visuals(page).count();
  await page.locator(".viz-gallery").getByRole("button", { name: label, exact: true }).click();
  await expect(visuals(page)).toHaveCount(before + 1);
  return visuals(page).nth(before);
}

/** Drags a field from the Fields pane into a role well of the selected visual. */
async function dropInWell(page: Page, table: string, field: string, role: string) {
  await expandTable(page, table);
  const tableItem = fieldsPane(page).locator("li[role=treeitem]", { has: page.locator(".field-table-btn", { hasText: new RegExp(`^${table}$`) }) });
  const row = tableItem.locator(".field-row", { has: page.locator(".field-name", { hasText: new RegExp(`^${field}$`) }) });
  // Onto the well's empty slot: dropping on a chip inserts before it.
  await row.dragTo(page.locator(`.well[data-role="${role}"] .well-empty`));
  await expect(page.locator(`.well[data-role="${role}"] .chip-name`, { hasText: field }).first()).toBeVisible();
}

/** Clicks a bar (by category index) of a column chart, through the chart's own geometry. */
async function clickBar(page: Page, visual: Locator, index: number, modifiers: ("Control" | "Shift")[] = []) {
  const chart = visual.locator(".cartesian");
  const at = await chart.evaluate((el, i) => {
    const c = (el as unknown as { chart: { getOption(): { series: { data: { value: number }[] }[] }; convertToPixel(f: object, v: number[]): number[] } }).chart;
    const v = c.getOption().series[0]!.data[i]!.value;
    // Chart pixels are before the canvas zoom; the bounding box is after it.
    const zoom = el.getBoundingClientRect().width / (el as HTMLElement).offsetWidth;
    return c.convertToPixel({ seriesIndex: 0 }, [i, v / 2]).map((x) => x * zoom);
  }, index);
  const box = (await chart.boundingBox())!;
  await page.mouse.move(box.x + at[0]!, box.y + at[1]!);
  for (const m of modifiers) await page.keyboard.down(m);
  await page.mouse.down();
  await page.mouse.up();
  for (const m of modifiers) await page.keyboard.up(m);
}

/** A card's value as a number of millions (cards show "$12.3M"). */
async function cardMillions(card: Locator): Promise<number> {
  const text = (await card.locator(".card-value").textContent()) ?? "";
  const m = /([\d.]+)([KMB]?)/.exec(text.replace(/,/g, ""));
  if (!m) return NaN;
  const scale = { K: 1e-3, M: 1, B: 1e3, "": 1e-6 }[m[2] as "K" | "M" | "B" | ""];
  return Number(m[1]) * scale;
}

const seriesCount = (visual: Locator) =>
  visual.locator(".cartesian").evaluate((el) => (el as unknown as { chart: { getOption(): { series: unknown[] } } }).chart.getOption().series.length);

test.setTimeout(180_000);
test.use({ actionTimeout: 15_000 });

test("cross-filter, highlight, Ctrl-click, slicers and edit interactions", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Interactivity");

  const card = await addVisual(page, "Card", { x: 140, y: 90 });
  await tick(page, "Sales", "Total Sales");
  await expect(card.locator(".card-value")).toHaveText(/^\$[\d.]+M$/);
  const total = await cardMillions(card);

  const column = await addVisual(page, "Clustered column chart", { x: 510, y: 170 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect(column.locator(".cartesian canvas")).toBeVisible();

  const line = await addVisual(page, "Line chart", { x: 1000, y: 540 });
  await tick(page, "Date", "YearMonth");
  await tick(page, "Sales", "Total Sales");
  await expect(line.locator(".cartesian canvas")).toBeVisible();

  const matrix = await addVisual(page, "Matrix", { x: 1010, y: 190 });
  await dropInWell(page, "Product", "Category", "rows");
  await dropInWell(page, "Product", "Subcategory", "rows");
  await dropInWell(page, "Date", "Year", "columns");
  await dropInWell(page, "Sales", "Total Sales", "values");
  await expect(matrix.locator(".matrix-row-head")).toHaveCount(7); // 6 categories + total
  await expect(matrix.locator("thead th", { hasText: "2025" })).toBeVisible();

  const slicer = await addVisual(page, "Slicer", { x: 120, y: 330 });
  await tick(page, "Store", "Region");
  await expect(slicer.locator(".slicer-item", { hasText: "North America" })).toBeVisible();
  await shot(page, "m4-01-page");

  // Click the first (largest) column: the card, line and matrix are filtered to it.
  await clickBar(page, column, 0);
  await expect.poll(() => cardMillions(card)).toBeLessThan(total * 0.9);
  const one = await cardMillions(card);
  await expect(matrix.locator(".matrix-row-head")).toHaveCount(2); // the category + total
  await expect(card.locator(".visual-badge")).toBeVisible();
  await shot(page, "m4-02-cross-filter");

  // Ctrl-click a second column: the selection grows.
  await clickBar(page, column, 1, ["Control"]);
  await expect.poll(() => cardMillions(card)).toBeGreaterThan(one);
  await expect(matrix.locator(".matrix-row-head")).toHaveCount(3);
  await shot(page, "m4-03-ctrl-click");

  // Clicking empty canvas clears it.
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  await page.mouse.click(pageBox.x + 4, pageBox.y + pageBox.height - 4);
  await expect.poll(() => cardMillions(card)).toBeCloseTo(total, 1);

  // A matrix row highlights the column chart: every bar gains a highlighted part.
  const camping = matrix.locator(".matrix-row-head", { hasText: "Camping" });
  const rowLabel = async () => {
    const b = (await camping.boundingBox())!;
    await page.mouse.click(b.x + b.width - 4, b.y + b.height / 2);
  };
  await rowLabel();
  await expect.poll(() => seriesCount(column)).toBe(2);
  await shot(page, "m4-04-highlight");
  await rowLabel();
  await expect.poll(() => seriesCount(column)).toBe(1);

  // The slicer filters everything else on the page.
  await slicer.locator(".slicer-item", { hasText: "North America" }).click();
  await expect.poll(() => cardMillions(card)).toBeLessThan(total * 0.9);
  const sliced = await cardMillions(card);
  await shot(page, "m4-05-slicer");

  // Edit interactions: the slicer no longer filters the card.
  await slicer.click({ position: { x: 20, y: 6 } });
  await page.getByRole("button", { name: "Edit interactions" }).click();
  const picker = card.locator(".interaction-picker");
  await expect(picker).toBeVisible();
  await expect(picker.getByRole("radio", { name: "Filter" })).toHaveAttribute("aria-checked", "true");
  await expect(column.locator(".interaction-picker").getByRole("radio", { name: "Highlight" })).toBeVisible();
  await shot(page, "m4-06-edit-interactions");
  await picker.getByRole("radio", { name: "None" }).click();
  await expect.poll(() => cardMillions(card)).toBeCloseTo(total, 1);
  await page.getByRole("button", { name: "Edit interactions" }).click();
  await expect(picker).toBeHidden();
  expect(sliced).toBeLessThan(total);

  // Undo brings the interaction back; the slicer choice is undoable too.
  await page.keyboard.press("Control+z");
  await expect.poll(() => cardMillions(card)).toBeCloseTo(sliced, 1);
  await slicer.getByRole("button", { name: "Clear selections" }).click();
  await expect.poll(() => cardMillions(card)).toBeCloseTo(total, 1);

  // Expand the matrix: Camping's subcategories appear under it.
  await matrix.getByRole("button", { name: "Expand Camping" }).click();
  await expect.poll(() => matrix.locator(".matrix-row-head").count()).toBeGreaterThan(7);
  await expect(matrix.locator(".matrix-row-head", { hasText: "Tents" })).toBeVisible();
  await shot(page, "m4-07-matrix-expanded");
  await matrix.getByRole("button", { name: "Collapse Camping" }).click();
  await expect(matrix.locator(".matrix-row-head")).toHaveCount(7);
  await save(page);
});

test("more visuals, focus mode, show as a table and export data", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Visuals");

  const pie = await addVisual(page, "Donut chart", { x: 200, y: 160 });
  await tick(page, "Store", "Channel");
  await tick(page, "Sales", "Total Sales");
  await expect(pie.locator(".cartesian canvas")).toBeVisible();

  const stacked = await addVisual(page, "Stacked column chart", { x: 640, y: 160 });
  await dropInWell(page, "Date", "Year", "category");
  await dropInWell(page, "Product", "Category", "series");
  await dropInWell(page, "Sales", "Total Sales", "values");
  await expect.poll(() => seriesCount(stacked)).toBe(6);

  const scatter = await addVisual(page, "Scatter chart", { x: 1000, y: 540 });
  await dropInWell(page, "Product", "Subcategory", "details");
  await dropInWell(page, "Sales", "Total Sales", "x");
  await dropInWell(page, "Sales", "Units", "y");
  await expect(scatter.locator(".cartesian canvas")).toBeVisible();

  const text = await addVisual(page, "Text box", { x: 200, y: 420 });
  await text.locator(".textbox").dblclick();
  await text.getByRole("textbox", { name: "Text" }).fill("Harbor & Pine — sales overview");
  await page.locator(".canvas-scroller").click({ position: { x: 4, y: 4 } });
  await expect(text.locator(".textbox")).toHaveText("Harbor & Pine — sales overview");

  const shape = await addVisual(page, "Shape", { x: 520, y: 440 });
  await expect(shape.locator("svg rect")).toBeVisible();

  const slicer = await addVisual(page, "Slicer", { x: 1150, y: 170 });
  await tick(page, "Date", "Date");
  await page.getByRole("tab", { name: "Format" }).click();
  await page.locator(".format-section", { hasText: "Slicer settings" }).getByRole("combobox", { name: "Style" }).selectOption("between");
  await expect(slicer.locator(".slicer-range input[type=date]").first()).toHaveValue("2022-01-01");
  await slicer.locator(".slicer-range input[type=date]").first().fill("2025-01-01");
  await slicer.locator(".slicer-range input[type=date]").first().press("Enter");
  await expect.poll(() => seriesCount(stacked)).toBe(6);
  await expect
    .poll(() => stacked.locator(".cartesian").evaluate((el) => ((el as unknown as { chart: { getOption(): { xAxis: { data: string[] }[] } } }).chart.getOption().xAxis[0]!.data.length)))
    .toBe(1);
  await page.getByRole("tab", { name: "Build" }).click();
  await shot(page, "m4-08-visuals");

  // Focus mode, then the data as a table, then back.
  await stacked.hover();
  await stacked.getByRole("button", { name: "Focus mode" }).click();
  const focus = page.getByRole("dialog", { name: "Focus mode" });
  await expect(focus.locator(".cartesian canvas")).toBeVisible();
  await focus.getByRole("button", { name: "Show as a table" }).click();
  const table = page.getByRole("dialog", { name: "Show as a table" }).locator(".data-table");
  await expect(table.locator("th", { hasText: "Total Sales" })).toBeVisible();
  await expect(table.locator("tbody tr")).toHaveCount(6);
  await shot(page, "m4-09-show-as-table");
  await page.getByRole("button", { name: "Back to report" }).click();
  await expect(page.locator(".focus-view")).toHaveCount(0);

  // Export the chart's data as CSV.
  const csv = `${tmp}/export/stacked.csv`;
  mkdirSync(`${tmp}/export`, { recursive: true });
  await page.evaluate((p) => ((window as unknown as { __chartedPick: () => string }).__chartedPick = () => p), csv);
  await stacked.hover();
  await stacked.getByRole("button", { name: "More options" }).click();
  await stacked.getByRole("menuitem", { name: "Export data" }).click();
  await expect.poll(() => existsSync(csv)).toBe(true);
  const lines = readFileSync(csv, "utf8").trim().split("\n");
  expect(lines[0]).toBe("Year,Category,Total Sales");
  expect(lines).toHaveLength(7);
  await page.evaluate(() => delete (window as unknown as { __chartedPick?: unknown }).__chartedPick);
  await save(page);
});

test("themes: built-in, a Power BI theme file, and a live Omarchy switch", async ({ page }) => {
  await openSample(page);
  await page.getByRole("tab", { name: "Interactivity" }).click();
  const reportPage = page.getByTestId("report-page");
  const bg = () => reportPage.evaluate((el) => getComputedStyle(el).backgroundColor);
  const omarchy = await bg();

  await page.getByRole("button", { name: "Themes" }).click();
  await page.getByRole("menuitem", { name: "Classic" }).click();
  await expect.poll(bg).toBe("rgb(255, 255, 255)");
  await shot(page, "m4-10-theme-classic");

  // Import a Power BI theme file.
  const file = `${tmp}/export/sunset.json`;
  mkdirSync(`${tmp}/export`, { recursive: true });
  writeFileSync(file, JSON.stringify({ name: "Sunset", dataColors: ["#B23A48", "#F2A541", "#3D5A80", "#7FB069"], background: "#FFF8F0", foreground: "#2B2D42", tableAccent: "#B23A48" }));
  await page.evaluate((p) => ((window as unknown as { __chartedPick: () => string }).__chartedPick = () => p), file);
  await page.getByRole("button", { name: "Themes" }).click();
  await page.getByRole("menuitem", { name: "Browse for themes…" }).click();
  await expect.poll(bg).toBe("rgb(255, 248, 240)");
  await page.evaluate(() => delete (window as unknown as { __chartedPick?: unknown }).__chartedPick);
  await page.getByRole("button", { name: "Themes" }).click();
  await expect(page.getByRole("menuitem", { name: "Sunset" })).toBeVisible();
  await shot(page, "m4-11-theme-imported");
  await page.getByRole("menuitem", { name: "Omarchy (follows the desktop)" }).click();
  await expect.poll(bg).toBe(omarchy);

  // Switch the desktop theme: the report follows it live.
  mkdirSync(themeDir, { recursive: true });
  const before = existsSync(`${themeDir}/colors.toml`) ? readFileSync(`${themeDir}/colors.toml`, "utf8") : null;
  writeFileSync(`${themeDir}/colors.toml`, 'mode = "light"\naccent = "#0f766e"\nbackground = "#f4f1ea"\nforeground = "#1c1917"\nblue = "#2563eb"\ngreen = "#15803d"\nred = "#b91c1c"\n');
  await expect.poll(bg, { timeout: 15_000 }).toBe("rgb(244, 241, 234)");
  await shot(page, "m4-12-omarchy-live");
  if (before !== null) writeFileSync(`${themeDir}/colors.toml`, before);
  await save(page);
});
