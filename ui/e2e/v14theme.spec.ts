// v1.4, the theme editor: data colours and title size change on the canvas while
// editing, OK keeps them as one step, and undo brings the old theme back.
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

/** The colour of a column chart's first bar, as ECharts draws it. */
const seriesColor = (chart: Locator) =>
  chart
    .locator(".cartesian")
    .evaluate((el) => String((el as unknown as { chart: { getOption(): { series: { data?: { itemStyle?: { color?: unknown } }[] }[] } } }).chart.getOption().series[0]?.data?.[0]?.itemStyle?.color ?? "").toLowerCase());
const titleSize = (chart: Locator) => chart.locator(".visual-title").evaluate((el) => getComputedStyle(el).fontSize);

test.setTimeout(180_000);
test.use({ actionTimeout: 15_000 });

test("theme editor: live preview of colours and title size, one undo step", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Theme");
  const chart = await addVisual(page, "Clustered column chart", { x: 400, y: 200 });
  await tick(page, "Product", "Category");
  await tick(page, "Sales", "Total Sales");
  await expect(chart.locator(".cartesian")).toBeVisible();
  await expect.poll(() => seriesColor(chart)).toMatch(/^#|rgb/);
  const before = await seriesColor(chart);
  const beforeSize = await titleSize(chart);
  expect(beforeSize).toBe("13px");

  await page.getByRole("button", { name: "Themes" }).click();
  await page.getByRole("menuitem", { name: "Customize current theme…" }).click();
  const editor = page.getByRole("dialog", { name: "Customize theme" });
  await expect(editor).toBeVisible();
  await editor.getByRole("textbox", { name: "Theme name" }).fill("Harbor red");
  await editor.getByLabel("Color 1", { exact: true }).fill("#d01c1c");
  // The canvas shows it before OK.
  await expect.poll(() => seriesColor(chart)).toBe("#d01c1c");
  await editor.getByRole("spinbutton", { name: "Title size" }).fill("24");
  await expect.poll(() => titleSize(chart)).toBe("24px");
  // Adding a colour keeps the first one first.
  const count = await editor.locator(".theme-colors li").count();
  await editor.getByRole("button", { name: "Add color" }).click();
  await expect(editor.locator(".theme-colors li")).toHaveCount(count + 1);
  expect(await seriesColor(chart)).toBe("#d01c1c");
  if (shots) await page.screenshot({ path: `${shots}/v14-03-theme-editor.png` });
  await editor.getByRole("button", { name: "OK" }).click();
  await expect(editor).toBeHidden();
  await expect.poll(() => seriesColor(chart)).toBe("#d01c1c");
  await expect.poll(() => titleSize(chart)).toBe("24px");
  await page.getByRole("button", { name: "Themes" }).click();
  await expect(page.getByRole("menuitem", { name: "Harbor red" })).toBeVisible();
  await page.keyboard.press("Escape");

  // One undo step brings the old theme back.
  await page.locator(".report-center").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("Control+z");
  await expect.poll(() => seriesColor(chart)).toBe(before);
  await expect.poll(() => titleSize(chart)).toBe(beforeSize);

  // Cancel drops the changes.
  await page.getByRole("button", { name: "Themes" }).click();
  await page.getByRole("menuitem", { name: "Customize current theme…" }).click();
  await editor.getByLabel("Color 1", { exact: true }).fill("#1c8a1c");
  await expect.poll(() => seriesColor(chart)).toBe("#1c8a1c");
  await editor.getByRole("button", { name: "Cancel" }).click();
  await expect.poll(() => seriesColor(chart)).toBe(before);
});
