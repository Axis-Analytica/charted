// The README's demos, recorded in one session on the sample:
//   build:   one request builds an in-depth customer page, then a scroll through it and a
//            slicer click that cross-filters it;
//   restyle: one short request turns the report purple, with new fonts.
// Writes clips.json: for each clip, the stretches of the video to keep, how much to speed
// each up ("auto" squeezes the agent's work to a few seconds), where to zoom and what
// caption to show; bin/demo-cut.mjs turns it into an ffmpeg edit.
import { writeFileSync } from "node:fs";
import { expect, type Page, test } from "@playwright/test";

const tmp = process.env.CHARTED_DEMO_TMP ?? "/tmp/charted-demo";
const BUILD =
  process.env.CHARTED_DEMO_REQUEST ??
  "Build an in-depth customer analysis page: a title and a one-line description at the top, slicers for year, segment, region and channel, headline cards for sales, customers, orders, sales per customer and average order value, sales by segment, sales by country, monthly sales by segment, a segment by product category matrix, and a table of the top 20 customers by sales.";
const RESTYLE = process.env.CHARTED_DEMO_RESTYLE ?? "Make it purple please, with Helvetica for the fonts.";

type Stretch = {
  from: number;
  to: number;
  speed: number | "auto";
  zoom?: { at: [number, number]; from: number; to: number };
  caption?: string;
};
const ZOOM = 3;

test("the agent builds a customer dashboard, then restyles it", async ({ page }) => {
  start = Date.now();
  const clips: Record<string, Stretch[]> = { build: [], restyle: [] };
  // Zoom in on the request box while it's typed, and back out as the agent starts.
  const box: [number, number] = [1430, 905];
  const zoomIn = { at: box, from: 1, to: ZOOM };
  const zoomOut = { at: box, from: ZOOM, to: 1 };

  await page.goto("/");
  await page.getByRole("button", { name: "Explore the sample" }).click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 60_000 });
  // The canvas and the Agent pane only.
  for (const [button, pane] of [["Filters pane", ".filters-pane"], ["Visualizations pane", ".viz-pane"], ["Fields pane", ".fields-pane"]] as const) {
    if (await page.locator(pane).isVisible()) await page.getByRole("button", { name: button, exact: true }).click();
    await expect(page.locator(pane)).toBeHidden();
  }
  await page.keyboard.press("Control+i");
  const pane = page.locator(".agent-pane");
  await expect(pane).toBeVisible();
  await page.waitForTimeout(1000);
  const r = await pane.getByRole("textbox", { name: "Ask the agent" }).boundingBox();
  if (r) box.splice(0, 2, r.x + r.width / 2, r.y + r.height / 2 + 15);

  // The build.
  const a = now() - 0.5;
  const work = await ask(page, BUILD, true);
  clips.build.push(
    { from: a, to: work.sent, speed: 1, zoom: zoomIn, caption: "One request…" },
    { from: work.sent, to: work.done, speed: "auto", zoom: zoomOut, caption: "…and Claude Code builds the whole page" },
  );
  const b = now();
  await page.waitForTimeout(2500);
  // Down the page and back, then a slicer.
  const scroller = page.locator(".canvas-scroller");
  const height = await scroller.evaluate((el) => el.scrollHeight - el.clientHeight);
  if (height > 40) {
    await smoothScroll(page, height, 5000);
    await page.waitForTimeout(1200);
    await smoothScroll(page, 0, 2000);
    await page.waitForTimeout(600);
  }
  const visuals = await page.getByTestId("visual").count();
  clips.build.push({ from: b, to: now(), speed: 1, caption: `${visuals} visuals: cards, charts, a matrix, a top 20` });
  const d = now();
  try {
    // The region slicer (the second dropdown is often a segment: one value left would
    // flatten the charts).
    const region = page.getByTestId("visual").filter({ has: page.locator(".slicer-title", { hasText: /region/i }) });
    const dropdown = (await region.count()) ? region.locator(".slicer-dropdown-btn").first() : page.locator(".slicer-dropdown-btn").last();
    await dropdown.hover({ timeout: 10_000 });
    await page.waitForTimeout(400);
    await dropdown.click({ timeout: 10_000 });
    await page.waitForTimeout(700);
    const item = page.locator(".slicer-item").nth(1);
    await item.hover({ timeout: 10_000 });
    await page.waitForTimeout(400);
    await item.click({ timeout: 10_000 });
    await page.waitForTimeout(900);
    await page.keyboard.press("Escape");
    await scroller.click({ position: { x: 6, y: 6 } });
    await page.waitForTimeout(3000);
  } catch {
    console.log("no slicer to click");
  }
  clips.build.push({ from: d, to: now(), speed: 1, caption: "Every visual cross-filters" });
  await page.screenshot({ path: `${tmp}/build.png` });

  // The restyle.
  const c = now() - 0.5;
  const re = await ask(page, RESTYLE, false);
  await page.waitForTimeout(3500);
  clips.restyle.push(
    { from: c, to: re.sent, speed: 1, zoom: zoomIn, caption: "Then: “Make it purple”" },
    { from: re.sent, to: re.done, speed: "auto", zoom: zoomOut, caption: "One sentence…" },
    { from: re.done, to: now(), speed: 1, caption: "…new colours, new fonts" },
  );
  await page.screenshot({ path: `${tmp}/restyle.png` });
  writeFileSync(`${tmp}/clips.json`, JSON.stringify({ size: [1600, 1000], ...clips }));
});

/** Types a request, sends it (agreeing to the first-use notice), and waits for the
 * reply; returns when it was sent and when it was done. */
async function ask(page: Page, text: string, first: boolean) {
  const pane = page.locator(".agent-pane");
  const box = pane.getByRole("textbox", { name: "Ask the agent" });
  await box.click();
  await box.pressSequentially(text, { delay: first ? 14 : 30 });
  await page.waitForTimeout(400);
  const turns = pane.locator(".agent-turn");
  const before = await turns.count();
  await pane.getByRole("button", { name: "Send", exact: true }).click();
  const sent = now();
  const agree = page.getByRole("dialog").getByRole("button", { name: "Agree and send" });
  const turn = turns.nth(before);
  await expect(agree.or(turn)).toBeVisible({ timeout: 15_000 });
  if (await agree.isVisible()) {
    await page.waitForTimeout(1200);
    await agree.click();
  }
  await expect(turn).toBeVisible();
  await expect(turn.locator(".agent-working")).toHaveCount(0, { timeout: 900_000 });
  await expect(turn.locator(".agent-text").last()).toBeVisible();
  return { sent, done: now() };
}

/** When the test (and so the video) started; times are seconds from it. */
let start = 0;
const now = () => (Date.now() - start) / 1000;

async function smoothScroll(page: Page, to: number, ms: number) {
  await page.locator(".canvas-scroller").evaluate(
    (el, [to, ms]) =>
      new Promise<void>((done) => {
        const from = el.scrollTop;
        const t0 = performance.now();
        const step = (t: number) => {
          const k = Math.min(1, (t - t0) / ms);
          el.scrollTop = from + (to - from) * (0.5 - Math.cos(Math.PI * k) / 2);
          if (k < 1) requestAnimationFrame(step);
          else done();
        };
        requestAnimationFrame(step);
      }),
    [to, ms] as const,
  );
}
