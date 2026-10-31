// The README's demo: one request in the Agent pane builds a dashboard on the sample,
// then a slicer click cross-filters it. Writes the moments to cut at to marks.json.
import { writeFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const tmp = process.env.CHARTED_DEMO_TMP ?? "/tmp/charted-demo";
const REQUEST =
  process.env.CHARTED_DEMO_REQUEST ??
  "Build an Executive overview page: a header text box with a bold title and a short italic subtitle in Playfair Display, cards for total sales, orders and units, monthly sales as a line chart, sales by category as a donut, sales by region as a bar chart, and a region slicer.";

test("the agent builds a dashboard", async ({ page }) => {
  const t0 = Date.now();
  const marks: Record<string, number> = {};
  const mark = (k: string) => (marks[k] = (Date.now() - t0) / 1000);
  await page.goto("/");
  await page.getByRole("button", { name: "Explore the sample" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 60_000 });
  await page.waitForTimeout(1500);
  // The canvas and the Agent pane only.
  for (const [button, pane] of [["Filters pane", ".filters-pane"], ["Visualizations pane", ".viz-pane"], ["Fields pane", ".fields-pane"]] as const) {
    if (await page.locator(pane).isVisible()) await page.getByRole("button", { name: button, exact: true }).click();
    await expect(page.locator(pane)).toBeHidden();
  }
  await page.keyboard.press("Control+i");
  const pane = page.locator(".agent-pane");
  await expect(pane).toBeVisible();
  const box = pane.getByRole("textbox", { name: "Ask the agent" });
  await box.click();
  mark("typing");
  await box.pressSequentially(REQUEST, { delay: 18 });
  await page.waitForTimeout(400);
  await pane.getByRole("button", { name: "Send", exact: true }).click();
  mark("sent");
  const agree = page.getByRole("dialog").getByRole("button", { name: "Agree and send" });
  const turn = pane.locator(".agent-turn").last();
  await expect(agree.or(turn)).toBeVisible({ timeout: 15_000 });
  if (await agree.isVisible()) {
    await page.waitForTimeout(1200);
    await agree.click();
  }
  await expect(turn).toBeVisible();
  await expect(turn.locator(".agent-working")).toHaveCount(0, { timeout: 480_000 });
  await expect(turn.locator(".agent-text").last()).toBeVisible();
  mark("done");
  await page.waitForTimeout(3000);
  // Cross-filter with the slicer it made.
  try {
    const dropdown = page.locator(".slicer-dropdown-btn").first();
    if (await dropdown.isVisible()) {
      await dropdown.hover({ timeout: 10_000 });
      await page.waitForTimeout(400);
      await dropdown.click({ timeout: 10_000 });
      await page.waitForTimeout(700);
    }
    const item = page.locator(".slicer-item").nth(1);
    await item.hover({ timeout: 10_000 });
    await page.waitForTimeout(500);
    await item.click({ timeout: 10_000 });
    mark("filtered");
    await page.waitForTimeout(900);
    await page.keyboard.press("Escape");
    await page.locator(".canvas-scroller").click({ position: { x: 6, y: 6 } });
    await page.waitForTimeout(3500);
  } catch {
    console.log("no slicer to click");
  }
  mark("end");
  writeFileSync(`${tmp}/marks.json`, JSON.stringify(marks));
  await page.screenshot({ path: `${tmp}/final.png` });
});
