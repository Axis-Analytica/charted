// v1.3, report building: the selection pane, formatting several visuals at once and the
// format painter, copy and paste, sync slicers, buttons and page navigators, and the
// PDF export's print layout.
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

async function clickPage(page: Page, at: { x: number; y: number }) {
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  const scale = pageBox.width / 1280;
  await page.mouse.click(pageBox.x + at.x * scale, pageBox.y + at.y * scale);
}

async function addVisual(page: Page, label: string, at: { x: number; y: number }): Promise<Locator> {
  await clickPage(page, at);
  const before = await visuals(page).count();
  await page.locator(".viz-gallery").getByRole("button", { name: label, exact: true }).click();
  await expect(visuals(page)).toHaveCount(before + 1);
  return visuals(page).nth(before);
}

async function cardMillions(card: Locator): Promise<number> {
  const text = (await card.locator(".card-value").textContent()) ?? "";
  const m = /([\d.]+)([KMB]?)/.exec(text.replace(/,/g, ""));
  if (!m) return NaN;
  const scale = { K: 1e-3, M: 1, B: 1e3, "": 1e-6 }[m[2] as "K" | "M" | "B" | ""];
  return Number(m[1]) * scale;
}

const layerNames = (page: Page) => page.locator(".selection-pane .layer-name").allTextContents();

test.setTimeout(240_000);
test.use({ actionTimeout: 15_000 });

test("selection pane: layers, hide and show, select hidden visuals, reorder", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Layers");
  const card = await addVisual(page, "Card", { x: 160, y: 100 });
  await tick(page, "Sales", "Total Sales");
  await expect(card.locator(".card-value")).toHaveText(/\$/);
  await addVisual(page, "Text box", { x: 600, y: 100 });
  await addVisual(page, "Shape", { x: 900, y: 300 });

  await page.getByRole("button", { name: "Selection pane" }).click();
  const pane = page.locator(".selection-pane");
  await expect(pane).toBeVisible();
  // Top layer first: the visual added last.
  expect(await layerNames(page)).toEqual(["Shape", "Text box", "Total Sales"]);

  // Selecting in the pane selects on the canvas, and the other way round.
  await pane.getByRole("button", { name: "Text box", exact: true }).click();
  await expect(visuals(page).nth(1)).toHaveAttribute("aria-selected", "true");
  await card.click({ position: { x: 20, y: 60 } });
  await expect(pane.locator(".layer.is-on .layer-name")).toHaveText("Total Sales");

  // Hide the card: it leaves the canvas but can still be selected and formatted.
  await pane.getByRole("button", { name: "Hide Total Sales" }).click();
  await expect(visuals(page)).toHaveCount(2);
  await pane.getByRole("button", { name: "Total Sales", exact: true }).click();
  await page.locator(".viz-pane").getByRole("tab", { name: "Format" }).click();
  await expect(page.locator(".format-section", { hasText: "Callout value" })).toBeVisible();
  await shot(page, "v13-01-selection-pane");
  await page.keyboard.press("Control+z");
  await expect(visuals(page)).toHaveCount(3);
  await page.keyboard.press("Control+Shift+z");
  await expect(visuals(page)).toHaveCount(2);
  await pane.getByRole("button", { name: "Show Total Sales" }).click();
  await expect(visuals(page)).toHaveCount(3);

  // Alt+↑ moves the card to the top; it is drawn in front.
  await pane.getByRole("button", { name: "Total Sales", exact: true }).focus();
  await page.keyboard.press("Alt+ArrowUp");
  await expect.poll(() => layerNames(page)).toEqual(["Shape", "Total Sales", "Text box"]);
  await page.keyboard.press("Alt+ArrowUp");
  await expect.poll(() => layerNames(page)).toEqual(["Total Sales", "Shape", "Text box"]);
  // Dragging a layer to the bottom.
  await pane.locator(".layer").nth(0).dragTo(pane.locator(".layer").nth(2), { targetPosition: { x: 20, y: 4 } });
  await expect.poll(() => layerNames(page)).toEqual(["Shape", "Total Sales", "Text box"]);
  const z = await card.evaluate((el) => Number((el as HTMLElement).style.zIndex));
  const zText = await visuals(page).nth(1).evaluate((el) => Number((el as HTMLElement).style.zIndex));
  expect(z).toBeGreaterThan(zText);
});
