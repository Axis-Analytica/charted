import { expect, type Page, test } from "@playwright/test";
import { readFileSync } from "node:fs";

const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const shots = process.env.CHARTED_SHOTS;

/** A table entry in the sidebar, by exact name. */
const table = (page: Page, name: string) => page.getByRole("tree", { name: "Tables" }).locator(".tree-label").getByText(name, { exact: true });

/** Opens the sample from the welcome screen unless a project is already open. */
async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  if (await explore.isVisible()) await explore.click();
  await expect(page.locator(".shell")).toBeVisible({ timeout: 30_000 });
  // Projects open in the Report view; these tests use the Data view.
  await page.getByRole("button", { name: "Data view" }).click();
  await expect(table(page, "Sales")).toBeVisible({ timeout: 30_000 });
}

test("welcome screen, shortcuts and command palette", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "charted" })).toBeVisible();
  await expect(page.getByText("Projects you open will appear here.")).toBeVisible();
  if (shots) await page.screenshot({ path: `${shots}/m0-welcome.png` });

  await page.keyboard.press("?");
  const dialog = page.getByRole("dialog", { name: "Keyboard shortcuts" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("Command palette")).toBeVisible();
  if (shots) await page.screenshot({ path: `${shots}/m0-shortcuts.png` });
  await page.keyboard.press("Escape");

  await page.keyboard.press("Control+k");
  await expect(page.getByRole("dialog", { name: "Commands" })).toBeVisible();
  await page.keyboard.type("sample");
  await expect(page.getByRole("option", { name: /Open the sample project/ })).toBeVisible();
  await page.keyboard.press("Escape");
});

test("open the sample, preview tables, rename with undo/redo, save", async ({ page }) => {
  await openSample(page);
  for (const name of ["Date", "Product", "Customer", "Store", "Targets"]) await expect(table(page, name)).toBeVisible();
  await expect(page.getByText("245,387 rows").first()).toBeVisible();
  await expect(page.locator(".preview-grid canvas").first()).toBeVisible();
  if (shots) await page.screenshot({ path: `${shots}/m0-tables-preview.png` });

  // Rename Customer -> Clients with F2.
  await table(page, "Customer").click();
  await page.keyboard.press("F2");
  const input = page.getByRole("textbox", { name: "New name" });
  await input.fill("Clients");
  await input.press("Enter");
  await expect(table(page, "Clients")).toBeVisible();
  await expect(page.getByText("Unsaved changes").first()).toBeVisible();

  await page.keyboard.press("Control+z");
  await expect(table(page, "Customer")).toBeVisible();
  await page.keyboard.press("Control+Shift+Z");
  await expect(table(page, "Clients")).toBeVisible();

  await page.keyboard.press("Control+s");
  await expect(page.getByText("All changes saved")).toBeVisible();
  const yaml = readFileSync(`${tmp}/docs/charted/Harbor & Pine/model.ossie.yaml`, "utf8");
  // Datasets are top-level list items; the table's own "Customer" column stays.
  expect(yaml).toMatch(/^- name: Clients$/m);
  expect(yaml).not.toMatch(/^- name: Customer$/m);

  // The load query is shown on request.
  await page.getByRole("button", { name: /Load query/ }).click();
  // Clients (formerly Customer) is still loaded from its CSV file.
  await expect(page.locator(".sql-panel")).toContainText("read_csv(");
  await expect(page.locator(".sql-panel")).toContainText("customer.csv");
});

test("duplicate names are rejected with a clear message", async ({ page }) => {
  await openSample(page);
  await table(page, "Store").click();
  await page.keyboard.press("F2");
  await page.getByRole("textbox", { name: "New name" }).fill("Sales");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("alert")).toContainText('"Sales" is already used');
  await expect(table(page, "Store")).toBeVisible();
});
