// v1.3, report building: the selection pane, formatting several visuals at once and the
// format painter, copy and paste, sync slicers, buttons and page navigators, and the
// PDF export's print layout.
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

async function fileMenu(page: Page, item: string) {
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: item }).click();
}

const layerNames = (page: Page) => page.locator(".selection-pane .layer-name").allTextContents();

test.setTimeout(240_000);
test.use({ actionTimeout: 15_000, permissions: ["clipboard-read", "clipboard-write"] });

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

test("format several visuals at once, and the format painter", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Painter");
  const fmt = page.locator(".viz-pane .format-pane");
  const a = await addVisual(page, "Text box", { x: 200, y: 80 });
  await page.locator(".viz-pane").getByRole("tab", { name: "Format" }).click();
  const setText = async (text: string) => {
    await fmt.getByRole("textbox", { name: "Text", exact: true }).fill(text);
    await fmt.getByRole("textbox", { name: "Text", exact: true }).press("Enter");
  };
  await setText("Alpha");
  await fmt.getByRole("spinbutton", { name: "Text size" }).fill("30");
  await fmt.getByRole("spinbutton", { name: "Text size" }).press("Enter");
  await expect(a.locator(".textbox")).toHaveCSS("font-size", "30px");
  const b = await addVisual(page, "Text box", { x: 200, y: 260 });
  await setText("Beta");
  await expect(b.locator(".textbox")).toHaveText("Beta");

  // Both selected: the shared options, with differing values shown as mixed.
  await a.click({ position: { x: 30, y: 20 } });
  await b.click({ position: { x: 30, y: 20 }, modifiers: ["Control"] });
  await expect(page.locator(".viz-pane").getByText("Formatting 2 visuals")).toBeVisible();
  await expect(fmt.getByRole("spinbutton", { name: "Text size" })).toHaveAttribute("placeholder", "Mixed");
  // One change formats both, in one undo step.
  await fmt.getByRole("checkbox", { name: "Bold" }).click();
  await expect(a.locator(".textbox")).toHaveCSS("font-weight", "700");
  await expect(b.locator(".textbox")).toHaveCSS("font-weight", "700");
  await shot(page, "v13-02-multi-format");
  await page.locator(".report-center").click({ position: { x: 4, y: 4 } });
  await page.keyboard.press("Control+z");
  await expect(a.locator(".textbox")).toHaveCSS("font-weight", "400");
  await expect(b.locator(".textbox")).toHaveCSS("font-weight", "400");

  // The format painter: Alpha's look onto Beta, which keeps its words.
  await a.click({ position: { x: 30, y: 20 } });
  const painter = page.getByRole("button", { name: /^Format painter/ });
  await painter.click();
  await expect(painter).toHaveAttribute("aria-pressed", "true");
  await b.click({ position: { x: 30, y: 20 } });
  await expect(b.locator(".textbox")).toHaveCSS("font-size", "30px");
  await expect(b.locator(".textbox")).toHaveText("Beta");
  await expect(painter).toHaveAttribute("aria-pressed", "false");

  // Double-clicked, it stays on for several visuals until Escape.
  const s1 = await addVisual(page, "Shape", { x: 700, y: 120 });
  await fmt.getByRole("combobox", { name: "Shape" }).selectOption("ellipse");
  await expect(s1.locator("ellipse")).toHaveCount(1);
  const s2 = await addVisual(page, "Shape", { x: 700, y: 330 });
  const s3 = await addVisual(page, "Shape", { x: 1000, y: 330 });
  await s1.click({ position: { x: 100, y: 60 } });
  await painter.dblclick();
  await expect(painter).toHaveAttribute("aria-pressed", "true");
  await s2.click({ position: { x: 100, y: 60 } });
  await s3.click({ position: { x: 100, y: 60 } });
  await expect(s2.locator("ellipse")).toHaveCount(1);
  await expect(s3.locator("ellipse")).toHaveCount(1);
  await expect(painter).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(painter).toHaveAttribute("aria-pressed", "false");

  // Ctrl+Alt+C copies a look, Ctrl+Alt+V pastes it onto everything selected.
  const c = await addVisual(page, "Text box", { x: 200, y: 440 });
  await a.click({ position: { x: 30, y: 20 } });
  await page.keyboard.press("Control+Alt+c");
  await c.click({ position: { x: 30, y: 20 } });
  await page.keyboard.press("Control+Alt+v");
  await expect(c.locator(".textbox")).toHaveCSS("font-size", "30px");
});

test("copy, cut and paste visuals: on the page, on another page, in another project", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Copies");
  const card = await addVisual(page, "Card", { x: 200, y: 120 });
  await tick(page, "Sales", "Total Sales");
  await expect(card.locator(".card-value")).toHaveText(/\$/);
  const text = await addVisual(page, "Text box", { x: 600, y: 120 });
  await card.click({ position: { x: 20, y: 60 } });
  await text.click({ position: { x: 20, y: 20 }, modifiers: ["Control"] });
  await page.keyboard.press("Control+c");
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("application/x-charted-visuals");

  // Pasted on the same page: offset, on top and selected.
  await page.keyboard.press("Control+v");
  await expect(visuals(page)).toHaveCount(4);
  const box = async (l: Locator) => (await l.boundingBox())!;
  const [c0, c1] = [await box(card), await box(visuals(page).nth(2))];
  expect(c1.x - c0.x).toBeGreaterThan(5);
  expect(c1.y - c0.y).toBeGreaterThan(5);
  await expect(visuals(page).nth(2).locator(".card-value")).toHaveText(/\$/);
  await expect(visuals(page).nth(2)).toHaveAttribute("aria-selected", "true");
  await expect(visuals(page).nth(3)).toHaveAttribute("aria-selected", "true");

  // Cut them: they go, and paste back.
  await page.keyboard.press("Control+x");
  await expect(visuals(page)).toHaveCount(2);
  await page.keyboard.press("Control+v");
  await expect(visuals(page)).toHaveCount(4);

  // On another page, at the same place.
  await page.keyboard.press("Escape");
  await card.click({ position: { x: 6, y: 6 } });
  await page.keyboard.press("Control+c");
  await newPage(page, "Elsewhere");
  await page.keyboard.press("Control+v");
  await expect(visuals(page)).toHaveCount(1);
  const pasted = visuals(page).first();
  await expect(pasted.locator(".card-value")).toHaveText(/\$/);
  expect(Math.abs((await box(pasted)).x - c0.x)).toBeLessThan(2);
  await shot(page, "v13-03-pasted");

  // In another project without these fields: the visual comes, its fields don't.
  await page.keyboard.press("Control+s");
  await expect(page.locator(".dirty-dot")).toHaveCount(0);
  await fileMenu(page, "Close project");
  await page.getByRole("button", { name: "New project…" }).click();
  const dialog = page.getByRole("dialog", { name: "New project" });
  await dialog.getByRole("textbox", { name: "Project name" }).fill("Paste target");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(page.locator(".topbar")).toContainText("Paste target");
  await page.locator(".report-center").click({ position: { x: 4, y: 4 } });
  await page.keyboard.press("Control+v");
  await expect(visuals(page)).toHaveCount(1);
  await expect(page.getByText("Some fields aren't in this model")).toBeVisible();
  await expect(visuals(page).first().locator(".visual-placeholder")).toBeVisible();
  await page.keyboard.press("Control+s");
  await expect(page.locator(".dirty-dot")).toHaveCount(0);
});

test("sync slicers: one choice filters every synced page, hidden or visible", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Sync A");
  const cardA = await addVisual(page, "Card", { x: 500, y: 100 });
  await tick(page, "Sales", "Total Sales");
  await expect(cardA.locator(".card-value")).toHaveText(/\$/);
  const total = await cardMillions(cardA);
  const slicer = await addVisual(page, "Slicer", { x: 140, y: 200 });
  await tick(page, "Store", "Region");
  await expect(slicer.locator(".slicer-item", { hasText: "North America" })).toBeVisible();
  await newPage(page, "Sync B");
  const cardB = await addVisual(page, "Card", { x: 500, y: 100 });
  await tick(page, "Sales", "Total Sales");
  await expect(cardB.locator(".card-value")).toHaveText(/\$/);

  // Sync the slicer to page B without showing it there.
  await page.getByRole("tab", { name: "Sync A" }).click();
  await slicer.click({ position: { x: 20, y: 6 } });
  await page.getByRole("button", { name: "Sync slicers pane" }).click();
  const pane = page.locator(".sync-pane");
  await expect(pane.getByRole("checkbox", { name: "Sync to Sync A" })).toBeChecked();
  await expect(pane.getByRole("checkbox", { name: "Sync to Sync B" })).not.toBeChecked();
  await pane.getByRole("checkbox", { name: "Sync to Sync B" }).click();
  await expect(pane.getByRole("checkbox", { name: "Sync to Sync B" })).toBeChecked();
  await expect(pane.getByRole("checkbox", { name: "Visible on Sync B" })).not.toBeChecked();
  await pane.getByText("Advanced options").click();
  await expect(pane.getByRole("textbox", { name: "Group name" })).toHaveValue("Region");
  await shot(page, "v13-04-sync-slicers");

  // A choice on page A filters page B too.
  await slicer.locator(".slicer-item", { hasText: "North America" }).click();
  await expect.poll(() => cardMillions(cardA)).toBeLessThan(total * 0.9);
  const sliced = await cardMillions(cardA);
  await page.getByRole("tab", { name: "Sync B" }).click();
  await expect(visuals(page)).toHaveCount(1);
  await expect.poll(() => cardMillions(cardB)).toBeCloseTo(sliced, 1);

  // Shown on page B, the copy has the same choice; clearing it there clears page A.
  await page.getByRole("tab", { name: "Sync A" }).click();
  await slicer.click({ position: { x: 20, y: 6 } });
  await pane.getByRole("checkbox", { name: "Visible on Sync B" }).click();
  await page.getByRole("tab", { name: "Sync B" }).click();
  await expect(visuals(page)).toHaveCount(2);
  const copy = page.locator('[data-type="slicer"]');
  await expect(copy.locator(".slicer-item.is-on", { hasText: "North America" })).toHaveCount(1);
  await copy.getByRole("button", { name: "Clear selections" }).click();
  await expect.poll(() => cardMillions(cardB)).toBeCloseTo(total, 1);
  await page.getByRole("tab", { name: "Sync A" }).click();
  await expect.poll(() => cardMillions(cardA)).toBeCloseTo(total, 1);
  // One undo step brings the choice back on both pages.
  await page.locator(".report-center").click({ position: { x: 4, y: 4 } });
  await page.keyboard.press("Control+z");
  await expect.poll(() => cardMillions(cardA)).toBeCloseTo(sliced, 1);
  await page.getByRole("tab", { name: "Sync B" }).click();
  await expect.poll(() => cardMillions(cardB)).toBeCloseTo(sliced, 1);
});

test("buttons and a page navigator: Ctrl+click while editing, click in reading view, hidden pages", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Details");
  await newPage(page, "Buttons");
  const fmt = page.locator(".viz-pane .format-pane");
  const button = await addVisual(page, "Button", { x: 200, y: 100 });
  await page.locator(".viz-pane").getByRole("tab", { name: "Format" }).click();
  await fmt.getByRole("combobox", { name: "Icon" }).selectOption("pageNavigation");
  await fmt.getByRole("textbox", { name: "Text", exact: true }).fill("Go to details");
  await fmt.getByRole("textbox", { name: "Text", exact: true }).press("Enter");
  await fmt.getByRole("button", { name: "Action" }).click();
  await fmt.getByRole("combobox", { name: "Destination" }).selectOption({ label: "Details" });
  await expect(button.getByRole("button", { name: "Go to details" })).toBeVisible();

  // A plain click selects it; Ctrl+click follows it.
  await button.click();
  await expect(button).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tab", { name: "Buttons" })).toHaveAttribute("aria-selected", "true");
  await button.click({ modifiers: ["Control"] });
  await expect(page.getByRole("tab", { name: "Details" })).toHaveAttribute("aria-selected", "true");

  // A Back button returns to the page before.
  const back = await addVisual(page, "Button", { x: 200, y: 100 });
  await fmt.getByRole("combobox", { name: "Icon" }).selectOption("back");
  await back.click({ modifiers: ["Control"] });
  await expect(page.getByRole("tab", { name: "Buttons" })).toHaveAttribute("aria-selected", "true");

  // A page navigator lists the pages; hidden ones drop out.
  const nav = await addVisual(page, "Page navigator", { x: 600, y: 400 });
  await expect(nav.getByRole("button", { name: "Details" })).toBeVisible();
  await expect(nav.getByRole("button", { name: "Buttons" })).toHaveAttribute("aria-current", "page");
  await page.getByRole("tab", { name: "Details" }).click();
  await page.locator(".viz-pane").getByRole("tab", { name: "Format" }).click();
  await page.locator(".viz-pane").getByRole("checkbox", { name: "Hide page" }).click();
  await page.getByRole("tab", { name: "Buttons" }).click();
  await expect(nav.getByRole("button", { name: "Details" })).toHaveCount(0);
  await shot(page, "v13-05-buttons");

  // Reading view: hidden pages leave the tabs, and a plain click acts.
  await page.getByRole("button", { name: "Reading view" }).click();
  await expect(page.locator(".viz-pane")).toHaveCount(0);
  await expect(page.getByRole("tab", { name: "Details" })).toHaveCount(0);
  await button.getByRole("button", { name: "Go to details" }).click();
  await expect(page.locator(".page-tab.is-on")).toHaveText("Details");
  await page.getByRole("tab", { name: "Buttons" }).click();
  const first = (await page.locator(".page-tab").first().textContent())!;
  await nav.locator(".page-nav-button").first().click();
  await expect(page.locator(".page-tab.is-on")).toHaveText(first);
  await page.getByRole("button", { name: "Edit report" }).click();
  await expect(page.locator(".viz-pane")).toBeVisible();
  await expect(page.getByRole("tab", { name: "Details" })).toBeVisible();
});

test("export to PDF: the print layout has every page not hidden, loaded, one per sheet", async ({ page }) => {
  await openSample(page);
  await newPage(page, "Print me");
  const card = await addVisual(page, "Card", { x: 200, y: 120 });
  await tick(page, "Sales", "Total Sales");
  await expect(card.locator(".card-value")).toHaveText(/\$/);
  await newPage(page, "Hidden detail");
  await page.locator(".viz-pane").getByRole("tab", { name: "Format" }).click();
  await page.locator(".viz-pane").getByRole("checkbox", { name: "Hide page" }).click();
  await expect(page.locator(".page-tab.is-hidden", { hasText: "Hidden detail" })).toHaveCount(1);
  const shown = await page.locator(".page-tab:not(.is-hidden)").count();
  expect(shown).toBeGreaterThan(1);

  await fileMenu(page, "Export to PDF…");
  const layout = page.locator(".print-root");
  await expect(layout).toBeVisible();
  await expect(layout).toHaveAttribute("data-print-ready", "", { timeout: 30_000 });
  await expect(layout.locator(".print-page")).toHaveCount(shown);
  await expect(layout.locator(".print-page", { hasText: "Hidden detail" })).toHaveCount(0);
  // Every visual has its data: nothing is still loading, and the card shows its value.
  await expect(layout.locator("[data-loading]")).toHaveCount(0);
  await expect(layout.locator(".print-page").nth(shown - 1).locator(".card-value")).toHaveText(/\$/);
  await shot(page, "v13-06-print-layout");

  const pdf = await page.pdf({ preferCSSPageSize: true, printBackground: true });
  const text = pdf.toString("latin1");
  const sheets = text.match(/\/Type\s*\/Page[^s]/g)?.length ?? 0;
  expect(sheets).toBe(shown);
  // Sheets are the report page's size: 1280 × 720 CSS pixels is 960 × 540 points.
  expect(text).toMatch(/\/MediaBox\s*\[\s*0\s+0\s+960(\.0+)?\s+540(\.0+)?\s*\]/);

  await layout.getByRole("button", { name: "Close" }).click();
  await expect(layout).toHaveCount(0);
});
