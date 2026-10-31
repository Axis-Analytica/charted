// v1.7, fonts: a bundled font for a text box, and a font of the user's own, added from a
// file and registered as a web font.
import { expect, type Locator, type Page, test } from "@playwright/test";

const FONT = `${process.cwd()}/../crates/core/tests/fixtures/fonts/NotoSansBassaVah-Regular.ttf`;

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
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

const visuals = (page: Page) => page.getByTestId("visual");

async function addVisual(page: Page, label: string, at: { x: number; y: number }): Promise<Locator> {
  const box = (await page.getByTestId("report-page").boundingBox())!;
  const scale = box.width / 1280;
  await page.mouse.click(box.x + at.x * scale, box.y + at.y * scale);
  const before = await visuals(page).count();
  await page.locator(".viz-gallery").getByRole("button", { name: label, exact: true }).click();
  await expect(visuals(page)).toHaveCount(before + 1);
  return visuals(page).nth(before);
}

const family = (l: Locator) => l.evaluate((el) => getComputedStyle(el).fontFamily);

test.setTimeout(120_000);

test("a text box in a bundled font, then in a font the user adds", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Fonts");
  const text = await addVisual(page, "Text box", { x: 300, y: 200 });
  await text.locator(".textbox").dblclick();
  await text.getByRole("textbox", { name: "Text" }).fill("**Harbor** & Pine");
  await page.locator(".canvas-scroller").click({ position: { x: 4, y: 4 } });
  await text.click();
  await page.getByRole("tab", { name: "Format" }).click();
  const font = page.locator(".format-section", { hasText: "Text" }).getByRole("combobox", { name: "Font" });
  await font.selectOption("Lora");
  await expect.poll(() => family(text.locator(".textbox"))).toContain("Lora Variable");
  await expect.poll(() => page.evaluate(() => document.fonts.check('16px "Lora Variable"'))).toBe(true);

  await page.evaluate((p) => ((window as unknown as { __chartedPick: () => string }).__chartedPick = () => p), FONT);
  await font.selectOption("Add a font…");
  await expect.poll(() => family(text.locator(".textbox"))).toContain("Noto Sans Bassa Vah");
  // It's listed with the user's fonts and loaded as a web font.
  await expect(font.locator("optgroup[label='Your fonts'] option")).toHaveText(["Noto Sans Bassa Vah"]);
  await expect.poll(() => page.evaluate(async () => (await document.fonts.load('16px "Noto Sans Bassa Vah"')).length)).toBeGreaterThan(0);

  await page.getByRole("tab", { name: "Fonts" }).focus();
  await page.keyboard.press("Delete");
  await expect(page.getByRole("tab", { name: "Fonts" })).toHaveCount(0);
});
