// The README's screenshots (bin/screenshots): the projects the checkpoint tests built,
// opened in Chromium against the real core with the desktop's Omarchy theme. Skipped
// unless CHARTED_README_SHOTS names the folder to write them to.
import { cpSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { expect, type Page, test } from "@playwright/test";

const out = process.env.CHARTED_README_SHOTS;
const built = process.env.CHARTED_BUILT ?? `${process.env.HOME}/Work/charted-checkpoints`;

test.skip(!out, "set CHARTED_README_SHOTS to write the README's screenshots");
test.setTimeout(240_000);
test.use({ actionTimeout: 20_000 });

const pick = (page: Page, path: string) => page.evaluate((p) => ((window as unknown as { __chartedPick: () => string }).__chartedPick = () => p), path);
const shot = (page: Page, name: string) => page.screenshot({ path: `${out}/${name}.png` });

/** Opens a copy of a project a checkpoint built (so the original isn't touched). */
async function open(page: Page, milestone: string) {
  const copy = `${mkdtempSync(`${tmpdir()}/charted-shots-`)}/Harbor & Pine`;
  cpSync(`${built}/${milestone}/Harbor & Pine (built by the tests)`, copy, { recursive: true, filter: (p) => !p.includes("/.cache") });
  await page.goto("/");
  await closeProject(page);
  await pick(page, copy);
  await page.getByRole("button", { name: "Open project…" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 60_000 });
}

async function closeProject(page: Page) {
  const welcome = page.getByRole("button", { name: "Open project…" });
  await expect(welcome.or(page.locator(".shell"))).toBeVisible();
  if (await welcome.isVisible()) return;
  await page.getByRole("button", { name: "File" }).click();
  await page.getByRole("menuitem", { name: "Close project" }).click();
  await expect(welcome).toBeVisible();
}

async function tab(page: Page, name: string) {
  await page.getByRole("tab", { name, exact: true }).click();
  await page.waitForTimeout(2500);
}

test("README screenshots", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("button", { name: "New project…" })).toBeVisible();
  await shot(page, "welcome");

  // The report: a cross-filter on the Interactivity page, then the other visuals.
  await open(page, "m4");
  await tab(page, "Interactivity");
  await page.locator(".matrix-row-head", { hasText: /^Camping$/ }).first().click();
  await page.waitForTimeout(2000);
  await shot(page, "report");
  await tab(page, "Visuals");
  // Search the visual gallery, and the Fields pane's menu.
  await page.getByRole("textbox", { name: "Search visuals" }).fill("chart");
  await page.locator(".fields-pane .field-table-btn", { hasText: /^Sales$/ }).click({ button: "right" });
  await page.waitForTimeout(500);
  await shot(page, "visuals");
  await page.keyboard.press("Escape");
  await page.getByRole("textbox", { name: "Search visuals" }).fill("");

  // Time intelligence and DAX: the Trends page with a measure in the formula bar.
  await open(page, "m6");
  await tab(page, "Trends");
  await page.locator(".fields-pane .field-name", { hasText: /^Sales YTD$/ }).click();
  await expect(page.locator(".formula-bar .dax-editor")).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  await shot(page, "dax");
  await page.keyboard.press("Escape");

  await page.keyboard.press("Control+3");
  await page.waitForTimeout(2500);
  await shot(page, "model");

  // Transform data: cleaning a messy file.
  await open(page, "m5");
  await page.keyboard.press("Control+4");
  await expect(page.locator(".qe")).toBeVisible();
  await page.locator(".qe-query", { hasText: "stores_raw" }).first().click();
  await page.waitForTimeout(2500);
  await shot(page, "query");

  // Importing a Power BI project, with its report of what didn't come across (from a
  // copy, so the paths shown are neutral).
  await page.keyboard.press("Control+1");
  await closeProject(page);
  const contoso = `${mkdtempSync(`${tmpdir()}/charted-shots-`)}/Contoso`;
  cpSync(new URL("../../crates/core/tests/fixtures/Contoso", import.meta.url).pathname, contoso, { recursive: true });
  await pick(page, `${contoso}/Contoso.pbip`);
  await page.getByRole("button", { name: "Open a bundle or Power BI project…" }).click();
  await expect(page.getByRole("dialog", { name: /^Imported/ })).toBeVisible({ timeout: 60_000 });
  await page.waitForTimeout(1500);
  await shot(page, "import");
});
