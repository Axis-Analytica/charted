import { expect, type Locator, type Page, test } from "@playwright/test";
import { mkdirSync, readFileSync, rmSync } from "node:fs";

const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const shots = process.env.CHARTED_SHOTS;

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  if (await explore.isVisible()) await explore.click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
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

async function addVisual(page: Page, label: string, at: { x: number; y: number }): Promise<Locator> {
  const pageBox = (await page.getByTestId("report-page").boundingBox())!;
  const scale = pageBox.width / 1280;
  await page.mouse.click(pageBox.x + at.x * scale, pageBox.y + at.y * scale);
  const before = await visuals(page).count();
  await page.locator(".viz-gallery").getByRole("button", { name: label, exact: true }).click();
  await expect(visuals(page)).toHaveCount(before + 1);
  return visuals(page).nth(before);
}

/** Starts a new measure and returns its editor (focused). */
async function newMeasure(page: Page) {
  await page.locator(".report-center").click({ position: { x: 10, y: 10 } });
  await page.keyboard.press("Control+m");
  const editor = page.locator(".formula-bar .dax-editor");
  await expect(editor).toBeVisible();
  // Replace the suggested "Measure = " with our own text.
  await editor.click();
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Delete");
  return editor;
}

/** Commits the measure in the formula bar and waits for it to land in the Fields pane. */
async function commitFormula(page: Page, name: string) {
  await page.locator(".formula-bar").getByRole("button", { name: "Commit" }).click();
  await expandTable(page, "Sales");
  await expect(fieldsPane(page).locator(".field-name", { hasText: new RegExp(`^${name}$`) })).toHaveCount(1, { timeout: 15_000 });
}

test.setTimeout(120_000);

test("model view: the star, a new relationship, Both refused as ambiguous", async ({ page }) => {
  await openSample(page);
  await page.keyboard.press("Control+3");
  await expect(page.locator(".model-card")).toHaveCount(6);
  await expect(page.locator(".model-edge")).toHaveCount(6);
  await expect(page.locator(".model-edge.is-inactive")).toHaveCount(1);
  await shot(page, "01-model-star");

  // Drag Targets[StoreKey] onto Store[StoreKey]: the dialog checks the keys and
  // suggests many-to-one.
  const col = (table: string, column: string) => page.locator(`.model-card[aria-label="${table}"] .model-col`, { hasText: new RegExp(`^${column}$`) });
  await col("Targets", "StoreKey").dragTo(col("Store", "StoreKey"));
  const dialog = page.getByRole("dialog", { name: "New relationship" });
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".rel-check")).toContainText("every key has a match");
  await expect(dialog.getByLabel("Cardinality")).toHaveValue("manyToOne");
  await shot(page, "02-new-relationship");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(page.locator(".model-edge")).toHaveCount(7);

  // Filtering both ways would give Sales two paths to Store (direct, and through
  // Date → Targets → Store): refused, as in Power BI.
  await page.locator(".model-rel", { hasText: "Targets[StoreKey] → Store[StoreKey]" }).dblclick();
  const edit = page.getByRole("dialog", { name: "Edit relationship" });
  await edit.getByLabel("Cross filter direction").selectOption("Both");
  await edit.getByRole("button", { name: "Save" }).click();
  await expect(edit.getByRole("alert")).toContainText("two active paths");
  await shot(page, "03-both-refused");
  await edit.getByRole("button", { name: "Cancel" }).click();
  await expect(edit).toBeHidden();

  // Moved cards stay where they are put, saved with the report.
  const head = page.locator('.model-card[aria-label="Targets"] .model-card-title');
  const box = (await head.boundingBox())!;
  await page.mouse.move(box.x + 20, box.y + 8);
  await page.mouse.down();
  await page.mouse.move(box.x + 20, box.y + 140, { steps: 6 });
  await page.mouse.up();
  await expect(page.getByRole("button", { name: "Undo Move table" })).toBeVisible();
  await save(page);
  const report = JSON.parse(readFileSync(`${tmp}/docs/charted/Harbor & Pine/report/report.json`, "utf8"));
  expect(Object.keys(report.diagram ?? {})).toHaveLength(6);
});

test("DAX editor: autocomplete, signature help, squiggles; USERELATIONSHIP and ALL side by side", async ({ page }) => {
  await openSample(page);

  // Typing offers functions; Enter takes the suggestion.
  await newMeasure(page);
  await page.keyboard.type("Ship Sales = calcul");
  const suggest = page.locator(".suggest-widget");
  await expect(suggest).toBeVisible();
  await expect(suggest.locator(".monaco-list-row").first()).toContainText("CALCULATE");
  await shot(page, "04-autocomplete");
  await page.keyboard.press("Enter");
  await page.keyboard.type("[Total Sales], USERELATIONSHIP ( Sales[ShipDateKey], ");
  await expect(page.locator(".parameter-hints-widget")).toContainText("ColumnName2");
  await shot(page, "05-signature-help");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+a");
  await page.keyboard.insertText("Ship Sales = CALCULATE ( [Total Sales], USERELATIONSHIP ( Sales[ShipDateKey], 'Date'[DateKey] ) )");
  await commitFormula(page, "Ship Sales");
  await expect(page.locator(".formula-bar .monaco-editor .view-lines")).toContainText("Ship Sales = CALCULATE");

  await newMeasure(page);
  await page.keyboard.insertText("All Years Sales = CALCULATE ( [Total Sales], ALL ( 'Date' ) )");
  await commitFormula(page, "All Years Sales");

  // A misspelt column is underlined, and the problem is spelled out.
  await newMeasure(page);
  await page.keyboard.insertText("Broken = SUM ( Sales[Amont] )");
  await expect(page.locator(".formula-bar .squiggly-error")).toBeVisible();
  await expect(page.locator(".formula-problem")).toContainText("Amont");
  await shot(page, "06-squiggle");
  await page.locator(".formula-bar").getByRole("button", { name: "Close formula bar" }).click();

  // Ship-date sales and all-years sales next to sales by order year.
  const table = await addVisual(page, "Table", { x: 640, y: 360 });
  await tick(page, "Date", "Year");
  await tick(page, "Sales", "Total Sales");
  await tick(page, "Sales", "Ship Sales");
  await tick(page, "Sales", "All Years Sales");
  await expect(table.locator("thead th")).toHaveText(["Year", "Total Sales", "Ship Sales", "All Years Sales"]);
  const rows = table.locator("tbody tr");
  await expect(rows.first()).toBeVisible();
  const cells = async (col: number) => (await rows.locator(`td:nth-child(${col})`).allTextContents()).map((s) => s.trim());
  const all = await cells(4);
  expect(new Set(all).size, "ALL ( 'Date' ) ignores the year").toBe(1);
  expect(await cells(3)).not.toEqual(await cells(2));
  await shot(page, "07-userelationship-and-all");
  await save(page);
});

test("a RELATED calculated column, PBIP export and an Ossie round trip", async ({ page }) => {
  await openSample(page);
  await page.keyboard.press("Control+2");
  await page.getByRole("tree", { name: "Tables" }).locator(".tree-label").getByText("Sales", { exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sales" })).toBeVisible();
  const columns = await page.locator(".preview-sub").textContent();

  await page.getByRole("button", { name: "New column" }).click();
  const editor = page.locator(".formula-bar .dax-editor");
  await expect(editor).toBeVisible();
  await editor.click();
  await page.keyboard.press("Control+a");
  await page.keyboard.insertText("List Value = Sales[Quantity] * RELATED ( Product[ListPrice] )");
  await page.locator(".formula-bar").getByRole("button", { name: "Commit" }).click();
  await expect(page.locator(".preview-sub")).not.toHaveText(columns ?? "");
  await expect(page.getByTestId("status-distinct")).toContainText(/List Value: [\d,]+ distinct values/);
  await page.getByRole("button", { name: "Column statistics" }).click();
  await expect(page.locator(".stats-title")).toHaveText("List Value");
  await shot(page, "08-calculated-column");

  // Export a Power BI project and look at the TMDL.
  const out = `${tmp}/exports`;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  await page.evaluate((dir) => {
    (window as unknown as { __chartedPick: (o: { saveAs?: string; directory?: boolean }) => string }).__chartedPick = (o) => (o.directory ? dir : `${dir}/${o.saveAs ?? "Harbor & Pine.ossie.yaml"}`);
  }, out);
  await page.getByRole("button", { name: "File" }).click();
  await page.getByRole("menuitem", { name: "Export Power BI project…" }).click();
  await page.getByRole("dialog", { name: "Export Power BI project" }).getByRole("button", { name: "Export" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Exported a Power BI project" })).toBeVisible();
  const tmdl = readFileSync(`${out}/Harbor & Pine.SemanticModel/definition/tables/Sales.tmdl`, "utf8");
  expect(tmdl).toContain("column 'List Value' = Sales[Quantity] * RELATED ( Product[ListPrice] )");
  expect(tmdl).toContain("partition Sales = m");
  if (shots) {
    const view = await page.context().newPage();
    const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
    await view.setContent(`<body style="margin:0;background:#111;color:#ddd"><pre style="font:13px/1.45 monospace;padding:16px 20px;tab-size:4">${esc(tmdl.split("\n").slice(0, 60).join("\n"))}</pre></body>`);
    await view.screenshot({ path: `${shots}/09-pbip-sales-tmdl.png` });
    await view.close();
  }

  // Export the model as Apache Ossie, import it back: nothing differs.
  await page.getByRole("button", { name: "File" }).click();
  await page.getByRole("menuitem", { name: "Export Apache Ossie model…" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Exported the model" })).toBeVisible();
  await page.getByRole("button", { name: "File" }).click();
  await page.getByRole("menuitem", { name: "Import Apache Ossie model…" }).click();
  const dialog = page.getByRole("dialog", { name: "Import Apache Ossie model" });
  await expect(dialog).toContainText("0 differences");
  await shot(page, "10-ossie-round-trip");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await save(page);
});
