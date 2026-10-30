// v1.4, row-level security: a role made in Manage roles, the report viewed as it
// (cards and slicers see only its rows), and back.
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
  await tab.dblclick();
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

async function cardMillions(card: Locator): Promise<number> {
  const text = (await card.locator(".card-value").textContent()) ?? "";
  const m = /([\d.]+)([KMB]?)/.exec(text.replace(/,/g, ""));
  if (!m) return NaN;
  const scale = { K: 1e-3, M: 1, B: 1e3, "": 1e-6 }[m[2] as "K" | "M" | "B" | ""];
  return Number(m[1]) * scale;
}

async function security(page: Page, item: string) {
  await page.getByRole("button", { name: "Security" }).click();
  await page.getByRole("menuitem", { name: item }).click();
}

test.setTimeout(180_000);
test.use({ actionTimeout: 15_000 });

test("roles: create one, view the report as it, stop viewing", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Security");
  const card = await addVisual(page, "Card", { x: 500, y: 100 });
  await tick(page, "Sales", "Total Sales");
  await expect(card.locator(".card-value")).toHaveText(/\$/);
  const total = await cardMillions(card);
  const slicer = await addVisual(page, "Slicer", { x: 140, y: 200 });
  await tick(page, "Store", "Region");
  await expect(slicer.locator(".slicer-item", { hasText: "Europe" })).toBeVisible();

  // Manage roles: a role that sees Asia Pacific's stores only (v11pq renames North America).
  await security(page, "Manage roles…");
  const dialog = page.getByRole("dialog", { name: "Manage roles" });
  await dialog.getByRole("button", { name: "New role" }).click();
  await dialog.getByRole("textbox", { name: "Role name" }).fill("Asia only");
  await dialog.getByRole("listbox", { name: "Tables" }).getByRole("option", { name: "Store" }).click();
  await dialog.locator(".roles-dax .monaco-editor").click();
  await page.keyboard.insertText('[Region] = "Asia Pacific"');
  await expect(dialog.locator(".roles-dax .view-lines")).toContainText("Asia Pacific");
  await expect(dialog.getByRole("listbox", { name: "Tables" }).getByRole("option", { name: "Store" }).getByLabel("Filtered")).toBeVisible();
  // A measure isn't a row condition: the check says so before saving.
  await dialog.getByRole("listbox", { name: "Tables" }).getByRole("option", { name: "Product" }).click();
  await dialog.locator(".roles-dax .monaco-editor").click();
  await page.keyboard.insertText("[Total Sales] > 0");
  await expect(dialog.getByRole("alert")).toContainText("condition on each row");
  await dialog.getByRole("button", { name: "Remove filter" }).click();
  if (shots) await page.screenshot({ path: `${shots}/v14-01-manage-roles.png` });
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog).toBeHidden();

  // View as the role: the card and the slicer see only its rows.
  await security(page, "View as…");
  const viewAs = page.getByRole("dialog", { name: "View as roles" });
  await viewAs.getByRole("checkbox", { name: "Asia only" }).check();
  await viewAs.getByRole("button", { name: "OK" }).click();
  await expect(page.locator(".view-as-bar")).toContainText("Viewing as: Asia only");
  await expect(slicer.locator(".slicer-item", { hasText: "Europe" })).toHaveCount(0);
  await expect(slicer.locator(".slicer-item", { hasText: "Asia Pacific" })).toBeVisible();
  await expect.poll(() => cardMillions(card)).toBeLessThan(total * 0.9);
  const north = await cardMillions(card);
  expect(north).toBeGreaterThan(0);
  if (shots) await page.screenshot({ path: `${shots}/v14-02-view-as.png` });

  // Stop viewing: everything is back.
  await page.locator(".view-as-bar").getByRole("button", { name: "Stop viewing" }).click();
  await expect(page.locator(".view-as-bar")).toHaveCount(0);
  await expect.poll(() => cardMillions(card)).toBeCloseTo(total, 1);
  await expect(slicer.locator(".slicer-item", { hasText: "Europe" })).toBeVisible();

  // The role is part of the model: undo removes it.
  await page.keyboard.press("Control+z");
  await page.getByRole("button", { name: "Security" }).click();
  await expect(page.getByRole("menuitem", { name: "View as…" })).toBeDisabled();
  await page.keyboard.press("Escape");
});
