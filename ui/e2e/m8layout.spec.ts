// M8, reshaping charted: panes float and dock, two views sit side by side (two report
// pages each keep their own page), toolbar buttons hide, and the agent does the same
// through its tools. Every change is a layout change, undone from the Layout menu.
import { expect, type Page, test } from "@playwright/test";

const shots = process.env.CHARTED_SHOTS;

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

async function layoutMenu(page: Page, item: string) {
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitem", { name: item, exact: true }).click();
}

const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const pick = (page: Page, path: string) => page.evaluate((p) => ((window as unknown as { __chartedPick: () => string }).__chartedPick = () => p), path);

const fields = (page: Page) => page.locator('.pane-host[data-pane="fields"]');

test.afterEach(async ({ page }) => {
  // Back to the Standard profile as it ships, for the specs after this one.
  await layoutMenu(page, "Standard");
  await layoutMenu(page, "Reset Standard");
  await expect(fields(page)).toHaveClass(/is-right/);
});

test("panes float and dock, and a layout change undoes", async ({ page }) => {
  await openSample(page);
  await expect(fields(page)).toHaveClass(/is-right/);
  await fields(page).locator(".pane-head").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Float" }).click();
  await expect(fields(page)).toHaveClass(/is-float/);
  // Dragged by its header.
  const head = fields(page).locator(".pane-head");
  const box = (await head.boundingBox())!;
  await page.mouse.move(box.x + 20, box.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x + 120, box.y + 70, { steps: 5 });
  await page.mouse.up();
  await expect.poll(async () => (await fields(page).boundingBox())!.x).toBeGreaterThan(box.x + 60);
  if (shots) await page.screenshot({ path: `${shots}/m8-float.png` });
  await fields(page).locator(".pane-head").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Dock on the left" }).click();
  await expect(fields(page)).toHaveClass(/is-left/);
  // Undo goes back one layout change at a time.
  await layoutMenu(page, "Undo layout change");
  await expect(fields(page)).toHaveClass(/is-float/);
});

test("two views side by side; two report pages keep their own page", async ({ page }) => {
  await openSample(page);
  await layoutMenu(page, "The model beside it");
  await expect(page.locator(".views .view-slot")).toHaveCount(2);
  await expect(page.locator(".view-slot.is-model .model-view, .view-slot.is-model .main")).not.toHaveCount(0);
  await layoutMenu(page, "Two report pages");
  await expect(page.locator(".report-view")).toHaveCount(2);
  const [first, second] = [page.locator(".view-slot").nth(0), page.locator(".view-slot").nth(1)];
  const pageTab = (slot: typeof first, opts: { selected?: boolean; name?: string } = {}) => slot.locator(".page-tabs").getByRole("tab", { ...opts, exact: true });
  const tabs = pageTab(second);
  const firstOn = (await pageTab(first, { selected: true }).textContent())!.trim();
  // A new page in the second report shows there; the first stays put.
  const before = await tabs.count();
  await second.getByRole("button", { name: "New page" }).click();
  await expect(tabs).toHaveCount(before + 1);
  const added = (await tabs.last().textContent())!.trim();
  await expect(pageTab(second, { selected: true })).toHaveText(added);
  await expect(pageTab(first, { selected: true })).toHaveText(firstOn);
  if (shots) await page.screenshot({ path: `${shots}/m8-two-reports.png` });
  await page.keyboard.press("Control+z");
  await expect(tabs).toHaveCount(before);

  // F6 reaches the second view's canvas too.
  await first.locator(".report-center").click({ position: { x: 8, y: 8 } });
  for (let i = 0; i < 12 && !(await second.locator(".report-center").evaluate((el) => el.contains(document.activeElement))); i++) await page.keyboard.press("F6");
  await expect.poll(() => second.locator(".report-center").evaluate((el) => el.contains(document.activeElement))).toBe(true);

  // A bookmark shown from the panes moves the report view last used: the second one here.
  await second.getByRole("button", { name: "New page" }).click();
  await expect(tabs).toHaveCount(before + 1);
  await second.getByRole("button", { name: "New page" }).click();
  await expect(tabs).toHaveCount(before + 2);
  const [pa, pb] = (await tabs.allTextContents()).slice(-2).map((x) => x.trim());
  await pageTab(second, { name: pa }).click();
  await page.getByRole("button", { name: "Bookmarks pane" }).click();
  // Two views leave the panes narrow: they share a tabbed column.
  const bookmarks = page.locator(".bookmarks-pane");
  const tab = page.locator(".pane-switch").getByRole("tab", { name: "Bookmarks" });
  await expect(bookmarks.or(tab).first()).toBeVisible();
  if (await tab.isVisible()) await tab.click();
  const count = await bookmarks.locator(".bookmark-name").count();
  await bookmarks.getByRole("button", { name: "Add bookmark" }).click();
  await expect(bookmarks.locator(".bookmark-name")).toHaveCount(count + 1);
  await pageTab(second, { name: pb }).click();
  await expect(pageTab(second, { selected: true })).toHaveText(pb!);
  await bookmarks.locator(".bookmark-name").last().click();
  await expect(pageTab(second, { selected: true })).toHaveText(pa!);
  await expect(pageTab(first, { selected: true })).toHaveText(firstOn);
  // Undo the bookmark and the two pages.
  for (let i = 0; i < 3; i++) await page.keyboard.press("Control+z");
  await expect(tabs).toHaveCount(before);
  await expect(bookmarks.locator(".bookmark-name")).toHaveCount(count);
  await page.getByRole("button", { name: "Bookmarks pane" }).click();
  await layoutMenu(page, "Standard");
  await expect(page.locator(".report-view")).toHaveCount(1);
});

test("a profile exports to a file and imports as a new profile", async ({ page }) => {
  await openSample(page);
  await fields(page).locator(".pane-head").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Float" }).click();
  await expect(fields(page)).toHaveClass(/is-float/);
  await pick(page, `${tmp}/Mine.charted-workspace`);
  await layoutMenu(page, "Export Standard…");
  await expect(page.locator(".toast")).toContainText("Exported the layout profile");
  await layoutMenu(page, "Reset Standard");
  await expect(fields(page)).toHaveClass(/is-right/);
  await layoutMenu(page, "Import a profile…");
  // A copy under a free name, switched to, with the floating Fields pane.
  await expect(page.getByRole("button", { name: "Layout" })).toContainText("Standard (2)");
  await expect(fields(page)).toHaveClass(/is-float/);
  await layoutMenu(page, "Delete Standard (2)");
  await expect(page.getByRole("button", { name: "Layout" })).toContainText("Standard");
});

test("a toolbar button hides, and comes back from the Layout menu", async ({ page }) => {
  await openSample(page);
  const reading = page.getByRole("button", { name: "Reading view", exact: true });
  await reading.click({ button: "right" });
  await page.getByRole("menuitem", { name: /Hide Reading view/ }).click();
  await expect(reading).toHaveCount(0);
  // Still in the command palette.
  await page.keyboard.press("Control+k");
  await page.locator(".palette-input").fill("Reading view");
  await expect(page.locator(".palette")).toContainText("Reading view");
  await page.keyboard.press("Escape");
  await layoutMenu(page, "Buttons and menus (1 hidden)…");
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Show all" }).click();
  await expect(reading).toHaveCount(1);
  // File menu items and the other views' buttons hide from the same list.
  // The box follows the layout once the core has it.
  const pdf = dialog.getByRole("checkbox", { name: "Export to PDF…" });
  await pdf.click();
  await expect(pdf).not.toBeChecked();
  await dialog.getByRole("button", { name: "Done" }).click();
  await page.getByRole("button", { name: "File" }).click();
  await expect(page.getByRole("menuitem", { name: "Export Power BI project…" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Export to PDF…" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Model view" }).click();
  const fit = page.getByRole("button", { name: "Fit diagram" });
  await fit.click({ button: "right" });
  await page.getByRole("menuitem", { name: /Hide Fit diagram/ }).click();
  await expect(fit).toHaveCount(0);
  await layoutMenu(page, "Buttons and menus (2 hidden)…");
  await dialog.getByRole("button", { name: "Show all" }).click();
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(fit).toHaveCount(1);
  await page.getByRole("button", { name: "Report view" }).click();
});

test("the agent reshapes the layout, and one undo takes it back", async ({ page }) => {
  await openSample(page);
  await page.keyboard.press("Control+i");
  const pane = page.locator(".agent-pane");
  await expect(pane.locator(".agent-picker")).toContainText("Claude Code");
  await pane.getByRole("textbox", { name: "Ask the agent" }).fill("change my layout");
  await pane.getByRole("button", { name: "Send", exact: true }).click();
  const agree = page.getByRole("dialog").getByRole("button", { name: "Agree and send" });
  const turn = pane.locator(".agent-turn").last();
  await expect(agree.or(turn.getByText("change my layout"))).toBeVisible();
  if (await agree.isVisible()) await agree.click();
  await turn.getByRole("button", { name: "Allow" }).click();
  await expect(turn.locator(".agent-working")).toHaveCount(0, { timeout: 30_000 });
  await expect(fields(page)).toHaveClass(/is-float/);
  await expect(page.locator(".views .view-slot")).toHaveCount(2);
  await expect(page.getByRole("button", { name: "Align left", exact: true })).toHaveCount(0);
  await layoutMenu(page, "Undo layout change");
  await expect(fields(page)).toHaveClass(/is-right/);
  await expect(page.locator(".views")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Align left", exact: true })).toHaveCount(1);
  await page.getByRole("button", { name: "Agent pane", exact: true }).click();
});
